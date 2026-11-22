use crate::tty::Tty;
use alloc::{collections::VecDeque, vec::Vec};
use serverlib::{
    Error, Result,
    abi::{Message, SYS_READ, SYS_WRITE},
};

struct Output {
    client: Option<usize>,
    bytes: Vec<u8>,
    offset: usize,
}

struct Input {
    bytes: Vec<u8>,
    flight: Option<usize>,
    count: usize,
    offset: usize,
}

pub struct UartTransfers {
    pid: usize,
    output: VecDeque<Output>,
    output_id: Option<usize>,
    input: Option<Input>,
}

#[derive(Default)]
pub struct InputEffects {
    pub interrupts: usize,
    pub blocked: bool,
    pub processed: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CompletionAction {
    PumpOutput,
    WaitForEvent,
    InputFailed(Error),
}

impl UartTransfers {
    /// Create transfer bookkeeping for the configured real UART peer.
    pub fn new(pid: usize) -> Self {
        Self {
            pid,
            output: VecDeque::new(),
            output_id: None,
            input: None,
        }
    }

    /// Copy a client's complete write into owned storage before queueing it.
    pub fn write(
        &mut self,
        id: usize,
        count: usize,
        mut copy: impl FnMut(usize, usize, &mut [u8]) -> Result<usize>,
    ) -> Result<()> {
        self.output.try_reserve(1).map_err(|_| Error::ENOMEM)?;
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(count).map_err(|_| Error::ENOMEM)?;
        bytes.resize(count, 0);
        if copy(id, 0, &mut bytes)? != count {
            return Err(Error::EIO);
        }
        self.output.push_back(Output {
            client: Some(id),
            bytes,
            offset: 0,
        });
        Ok(())
    }

    /// Submit at most one output buffer, keeping its backing storage frozen until completion.
    pub fn pump(
        &mut self,
        mut submit: impl FnMut(usize, &Message) -> Result<usize>,
        mut reply: impl FnMut(usize, Result<usize>) -> Result<()>,
    ) -> Result<()> {
        if self.output_id.is_some() {
            return Ok(());
        }
        while let Some(output) = self.output.front() {
            let message = Message {
                operation: SYS_WRITE,
                args: [
                    0,
                    output.bytes[output.offset..].as_ptr() as usize,
                    output.bytes.len() - output.offset,
                    0,
                    0,
                    0,
                ],
                ..Message::default()
            };
            match submit(self.pid, &message) {
                Ok(id) => {
                    self.output_id = Some(id);
                    return Ok(());
                }
                Err(error) => {
                    if let Some(client) = output.client {
                        reply(client, Err(error))?;
                        self.output.pop_front();
                    } else {
                        return Ok(()); // Retain echo until a later event.
                    }
                }
            }
        }
        Ok(())
    }

    /// Correlate consumed completions before releasing or changing submitted storage.
    pub fn complete(
        &mut self,
        id: usize,
        result: Result<usize>,
        mut reply: impl FnMut(usize, Result<usize>) -> Result<()>,
    ) -> Result<CompletionAction> {
        if self.output_id == Some(id) {
            self.output_id = None;
            let output = self.output.front_mut().ok_or(Error::EIO)?;
            let remaining = output.bytes.len() - output.offset;
            let result = result.and_then(|count| {
                if count > remaining || (count == 0 && remaining != 0) {
                    Err(Error::EIO)
                } else {
                    Ok(count)
                }
            });
            if let Some(client) = output.client {
                reply(client, result)?;
                self.output.pop_front(); // Application retries its own short write.
            } else {
                match result {
                    Ok(count) => {
                        output.offset += count;
                        if output.offset == output.bytes.len() {
                            self.output.pop_front();
                        }
                    }
                    Err(_) => return Ok(CompletionAction::WaitForEvent),
                }
            }
            return Ok(CompletionAction::PumpOutput);
        }
        let input = self.input.as_mut().ok_or(Error::ESRCH)?;
        if input.flight != Some(id) {
            return Err(Error::ESRCH);
        }
        input.flight = None;
        input.count = match result {
            Ok(count) => count,
            Err(error) => {
                self.input = None;
                return Ok(CompletionAction::InputFailed(error));
            }
        };
        if input.count == 0 || input.count > input.bytes.len() {
            return Err(Error::EIO);
        }
        Ok(CompletionAction::PumpOutput)
    }

    /// Submit one owned input batch when the caller has activated single-reader input ownership.
    pub fn start_input(
        &mut self,
        mut submit: impl FnMut(usize, &Message) -> Result<usize>,
    ) -> Result<()> {
        if self.input.is_some() {
            return Ok(());
        }
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(256).map_err(|_| Error::ENOMEM)?;
        bytes.resize(256, 0);
        let message = Message {
            operation: SYS_READ,
            args: [0, bytes.as_mut_ptr() as usize, bytes.len(), 0, 0, 0],
            ..Message::default()
        };
        let id = submit(self.pid, &message)?;
        self.input = Some(Input {
            bytes,
            flight: Some(id),
            count: 0,
            offset: 0,
        });
        Ok(())
    }

    /// Process completed input while retaining bytes that cannot yet be buffered or echoed.
    pub fn process_input(&mut self, tty: &mut Tty) -> InputEffects {
        let mut effects = InputEffects::default();
        let Some(input) = self.input.as_mut() else {
            return effects;
        };
        if input.flight.is_some() {
            return effects;
        }
        let mut echo = Vec::new();
        while input.offset < input.count {
            let byte = input.bytes[input.offset];
            let needs_echo =
                !tty.is_interrupt(byte) && tty.termios().c_lflag & serverlib::tty::ECHO != 0;
            if needs_echo && (self.output.try_reserve(1).is_err() || echo.try_reserve(1).is_err()) {
                effects.blocked = true;
                break;
            }
            match tty.receive_byte(byte) {
                Ok(effect) => {
                    input.offset += 1;
                    effects.processed += 1;
                    effects.interrupts += usize::from(effect.interrupt);
                    if let Some(byte) = effect.echo {
                        echo.push(byte);
                    }
                }
                Err(_) => {
                    effects.blocked = true;
                    break;
                }
            }
        }
        if !echo.is_empty() {
            self.output.push_back(Output {
                client: None,
                bytes: echo,
                offset: 0,
            });
        }
        if input.offset == input.count {
            self.input = None;
        }
        effects
    }
}

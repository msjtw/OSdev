use crate::{authorize, drain, uart::Uart};
use alloc::{collections::VecDeque, vec::Vec};
use serverlib::{
    Error, Result,
    abi::{KERNEL_SENDER_ID, SYS_READ, SYS_WRITE},
    ipc::Request,
    uart::POLL,
};

#[derive(Clone, Copy, Debug)]
struct PendingRead {
    id: usize,
    count: usize,
}

#[derive(Debug)]
struct PendingWrite {
    id: usize,
    bytes: Vec<u8>,
}

#[derive(Debug)]
pub struct Service {
    tty_pid: Option<usize>,
    reads: VecDeque<PendingRead>,
    writes: VecDeque<PendingWrite>,
    scratch: Vec<u8>,
}

impl Service {
    /// Create the actual UART service with its optional configured TTY peer.
    pub fn new(tty_pid: Option<usize>) -> Self {
        Self {
            tty_pid,
            reads: VecDeque::new(),
            writes: VecDeque::new(),
            scratch: Vec::new(),
        }
    }

    /// Accept requests without waiting for hardware, progressing work before polling acknowledgement.
    pub fn accept(
        &mut self,
        request: Request,
        uart: &Uart,
        mut copy: impl FnMut(usize, usize, &mut [u8]) -> Result<usize>,
        mut reply: impl FnMut(usize, Result<usize>, Option<&[u8]>) -> Result<()>,
    ) -> Result<()> {
        let message = request.message;
        authorize(&message, self.tty_pid)?;
        let count = message.args[2];
        match message.operation {
            POLL => {
                self.progress(uart, &mut reply)?;
                reply(request.id, Ok(0), None)
            }
            SYS_READ if message.sender == KERNEL_SENDER_ID || count == 0 => {
                let result = drain(uart, count, &mut self.scratch);
                let data = if result.is_ok() {
                    Some(self.scratch.as_slice())
                } else {
                    None
                };
                Self::finish(&mut reply, request.id, result, data)
            }
            SYS_READ => {
                self.reads.try_reserve(1).map_err(|_| Error::ENOMEM)?;
                self.reads.push_back(PendingRead {
                    id: request.id,
                    count,
                });
                Ok(())
            }
            SYS_WRITE if count == 0 => reply(request.id, Ok(0), None),
            SYS_WRITE => {
                self.writes.try_reserve(1).map_err(|_| Error::ENOMEM)?;
                let mut bytes = Vec::new();
                bytes.try_reserve_exact(count).map_err(|_| Error::ENOMEM)?;
                bytes.resize(count, 0);
                if copy(request.id, 0, &mut bytes)? != count {
                    return Err(Error::EIO);
                }
                self.writes.push_back(PendingWrite {
                    id: request.id,
                    bytes,
                });
                Ok(())
            }
            _ => Err(Error::ENOSYS),
        }
    }

    /// Progress owned writes and FIFO reads without waiting for input or transmitter readiness.
    pub fn progress(
        &mut self,
        uart: &Uart,
        mut reply: impl FnMut(usize, Result<usize>, Option<&[u8]>) -> Result<()>,
    ) -> Result<()> {
        while let Some(write) = self.writes.front() {
            // QEMU completes direct output here, not when the request is accepted.
            uart.write(&write.bytes);
            Self::finish(&mut reply, write.id, Ok(write.bytes.len()), None)?;
            self.writes.pop_front();
        }
        while let Some(read) = self.reads.front().copied() {
            if !uart.data_ready() {
                break;
            }
            let result = drain(uart, read.count, &mut self.scratch);
            let data = if result.is_ok() {
                Some(self.scratch.as_slice())
            } else {
                None
            };
            Self::finish(&mut reply, read.id, result, data)?;
            self.reads.pop_front();
        }
        Ok(())
    }

    /// Complete a client copy error locally rather than making it a polling-operation failure.
    fn finish(
        reply: &mut impl FnMut(usize, Result<usize>, Option<&[u8]>) -> Result<()>,
        id: usize,
        result: Result<usize>,
        data: Option<&[u8]>,
    ) -> Result<()> {
        match reply(id, result, data) {
            Ok(()) => Ok(()),
            Err(error) => reply(id, Err(error), None),
        }
    }
}

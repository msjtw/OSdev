use alloc::collections::VecDeque;
use serverlib::{
    Error, Result,
    tty::{ECHO, ICANON, ICRNL, ISIG, Termios, VINTR},
};

#[derive(Debug, Default, PartialEq, Eq)]
pub struct InputEffect {
    pub echo: Option<u8>,
    pub interrupt: bool,
}

#[derive(Debug)]
pub struct Tty {
    termios: Termios,
    input: VecDeque<u8>,
    canonical_readable: usize,
    pub session_id: Option<usize>,
    pub foreground_pgid: Option<usize>,
}

impl Default for Tty {
    /// Preserve the existing terminal policy without imposing defaults on the shared ABI.
    fn default() -> Self {
        let mut c_cc = [0; 19];
        c_cc[VINTR] = 3;
        Self {
            termios: Termios {
                c_iflag: ICRNL,
                c_oflag: 0,
                c_cflag: 0,
                c_lflag: ISIG | ICANON | ECHO,
                c_line: 0,
                c_cc,
            },
            input: VecDeque::new(),
            canonical_readable: 0,
            session_id: None,
            foreground_pgid: None,
        }
    }
}

impl Tty {
    /// Recognize a non-buffering interrupt before reserving input or echo storage.
    pub fn is_interrupt(&self, mut byte: u8) -> bool {
        if self.termios.c_iflag & ICRNL != 0 && byte == b'\r' {
            byte = b'\n';
        }
        self.termios.c_lflag & ISIG != 0 && byte == self.termios.c_cc[VINTR]
    }

    /// Expose contiguous readable storage for a synchronous reply without allocating.
    pub fn readable_slice(&mut self, count: usize) -> &[u8] {
        let count = count.min(self.readable());
        &self.input.make_contiguous()[..count]
    }
    /// Report only requested readiness bits without consuming input or promising write completion.
    pub fn poll(&self, events: serverlib::tty::PollEvents) -> serverlib::tty::PollEvents {
        use serverlib::tty::{POLLIN, POLLOUT};
        (events & POLLOUT)
            | if self.readable() != 0 {
                events & POLLIN
            } else {
                0
            }
    }
    /// Update foreground state only after live-caller and group validation succeeds.
    pub fn set_foreground(
        &mut self,
        id: usize,
        pgid: usize,
        caller_session: impl FnOnce(usize) -> Result<usize>,
        validate: impl FnOnce(usize, usize, usize) -> Result<()>,
    ) -> Result<()> {
        let sid = caller_session(id)?;
        if sid == 0 || self.session_id != Some(sid) {
            return Err(Error::EPERM);
        }
        validate(id, sid, pgid)?;
        self.foreground_pgid = Some(pgid);
        Ok(())
    }

    /// Coalesce interrupt effects into SIGINT delivery when a foreground terminal is assigned.
    pub fn deliver_interrupts(
        &self,
        count: usize,
        send: impl FnOnce(usize, usize) -> Result<()>,
    ) -> Result<()> {
        if count != 0 {
            if let (Some(sid), Some(pgid)) = (self.session_id, self.foreground_pgid) {
                send(sid, pgid)?;
            }
        }
        Ok(())
    }
    /// Return the current terminal settings.
    pub fn termios(&self) -> Termios {
        self.termios
    }

    /// Replace settings and report whether buffered input changed readability.
    pub fn set_termios(&mut self, settings: Termios) -> bool {
        let before = self.readable();
        self.termios = settings;
        before != self.readable()
    }

    /// Count bytes currently available under the selected canonical or raw policy.
    pub fn readable(&self) -> usize {
        if self.termios.c_lflag & ICANON != 0 {
            self.canonical_readable
        } else {
            self.input.len()
        }
    }

    /// Apply input translation and signal/echo policy without performing external effects.
    pub fn receive_byte(&mut self, mut byte: u8) -> Result<InputEffect> {
        if self.termios.c_iflag & ICRNL != 0 && byte == b'\r' {
            byte = b'\n';
        }
        if self.termios.c_lflag & ISIG != 0 && byte == self.termios.c_cc[VINTR] {
            return Ok(InputEffect {
                echo: None,
                interrupt: true,
            });
        }
        self.input.try_reserve(1).map_err(|_| Error::ENOMEM)?;
        self.input.push_back(byte);
        if byte == b'\n' {
            self.canonical_readable = self.input.len();
        }
        Ok(InputEffect {
            echo: (self.termios.c_lflag & ECHO != 0).then_some(byte),
            interrupt: false,
        })
    }

    /// Copy readable bytes into existing storage without consuming buffered input.
    pub fn copy_readable(&self, destination: &mut [u8]) -> usize {
        let count = destination.len().min(self.readable());
        for (destination, source) in destination[..count].iter_mut().zip(self.input.iter()) {
            *destination = *source;
        }
        count
    }

    /// Commit consumption of bytes previously prepared for a successful read reply.
    pub fn consume(&mut self, count: usize) {
        assert!(count <= self.readable());
        self.input.drain(..count);
        self.canonical_readable = self.canonical_readable.saturating_sub(count);
    }

    /// Consume readable bytes into storage already allocated by the caller.
    pub fn read_into(&mut self, destination: &mut [u8]) -> usize {
        let count = self.copy_readable(destination);
        self.consume(count);
        count
    }
}

use alloc::{
    collections::VecDeque,
    sync::Arc,
    vec::{self, Vec},
};
use spin::Once;

use crate::{
    KERNEL,
    drivers::uart::{UART_DRIVER, UartDriver},
    lock::IntMutex,
    process::{
        ProcState, Process,
        fd::{Errno, FileOps, POLLIN, POLLOUT, PollEvents},
        signals::{self, SIGINT},
    },
    structures::RingBuffer,
    virtmemory::{copy_in, copy_in_cont, copy_out, copy_out_cont},
};

const ICRNL: u32 = 0o000400;
const ECHO: u32 = 0o000010;
const ICANON: u32 = 0o000002;
const ISIG: u32 = 0o000001;
const VINTR: usize = 0;
const INPUT_QUEUE_CAPACITY: usize = 256;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct Termios {
    c_iflag: u32,
    c_oflag: u32,
    c_cflag: u32,
    c_lflag: u32,
    c_line: u8,
    c_cc: [u8; 19],
}

impl Default for Termios {
    fn default() -> Self {
        let mut c_cc = [0; 19];
        c_cc[VINTR] = 3;
        Self {
            c_iflag: ICRNL,
            c_oflag: 0,
            c_cflag: 0,
            c_lflag: ISIG | ICANON | ECHO,
            c_line: 0,
            c_cc,
        }
    }
}

// input buffor with cannocical line policy
#[derive(Debug, Default)]
struct TtyInput {
    bytes: VecDeque<u8>,
    canonical_readable: usize,
}

impl TtyInput {
    fn push(&mut self, byte: u8) -> bool {
        self.bytes.push_back(byte);
        if byte == b'\n' {
            self.canonical_readable = self.bytes.len();
        }
        true
    }

    fn readable(&self, canonical: bool) -> usize {
        if canonical {
            self.canonical_readable
        } else {
            self.bytes.len()
        }
    }

    fn read(&mut self, len: usize, canonical: bool) -> alloc::vec::Vec<u8> {
        let count = len.min(self.readable(canonical));
        let mut bytes = alloc::vec::Vec::with_capacity(count);
        for _ in 0..count {
            bytes.push(self.bytes.pop_front().unwrap());
        }
        self.canonical_readable = self.canonical_readable.saturating_sub(count);
        bytes
    }
}

#[derive(Debug)]
pub(crate) struct TTY {
    termios: IntMutex<Termios>,
    session_id: IntMutex<Option<usize>>, // controlling session id
    foreground_pgid: IntMutex<Option<usize>>, // foreground process group id
    input: IntMutex<TtyInput>,
    transport: &'static UartDriver,
}

static TTY: Once<Arc<TTY>> = Once::new();

impl TTY {
    fn new() -> Self {
        Self {
            termios: IntMutex::new(Termios::default()),
            session_id: IntMutex::new(None),
            foreground_pgid: IntMutex::new(None),
            input: IntMutex::new(TtyInput::default()),
            transport: &UART_DRIVER,
        }
    }

    pub fn get() -> Arc<Self> {
        TTY.call_once(|| Arc::new(Self::new()));
        Arc::clone(TTY.get().unwrap())
    }

    pub fn set_controlling_session(sid: usize) {
        let tty = Self::get();
        *tty.session_id.lock() = Some(sid);
        *tty.foreground_pgid.lock() = Some(sid);
    }

    pub fn is_controlling_terminal(proc: &Process) -> bool {
        proc.sid.is_some() && proc.sid == *Self::get().session_id.lock()
    }

    fn input_channel(&self) -> usize {
        self as *const Self as usize
    }

    // Drain input from the UART transport into this terminal.
    pub fn poll_input(&self) {
        self.transport.drain_input(|bytes| self.receive_byte(bytes));
    }

    // receive input bytes and apply line policy
    pub fn receive_byte(&self, mut byte: u8) {
        let termios = *self.termios.lock();
        if termios.c_iflag & ICRNL != 0 && byte == b'\r' {
            byte = b'\n';
        }

        // are signals enabled
        if termios.c_lflag & ISIG != 0 && byte == termios.c_cc[VINTR] {
            if let Some(foreground_pgid) = *self.foreground_pgid.lock() {
                let signal_bit = signals::signal_bit(SIGINT).unwrap();
                let mut kernel = KERNEL.get().unwrap().lock();
                for proc in &mut kernel.process_table {
                    unsafe { proc.lock.lock_manual() };
                    if proc.pgid == Some(foreground_pgid) {
                        // mark signal and wakeup process
                        proc.pending_signals |= signal_bit;
                        if proc.state == ProcState::Sleeping {
                            proc.state = ProcState::Runnable;
                        }
                    }
                    unsafe { proc.lock.unlock_manual() };
                }
            }
            return;
        }

        let mut input = self.input.lock();
        input.push(byte);

        if termios.c_lflag & ECHO != 0 {
            self.transport.write(&[byte]);
        }

        let canonical = termios.c_lflag & ICANON != 0;
        if input.readable(canonical) != 0 {
            // there is somethin to read so wakeup procees waiting for input
            KERNEL
                .get()
                .unwrap()
                .lock()
                .wakeup(Some(self.input_channel()), false);
        }
    }

    fn read_blocking(&self, proc: &mut Process, len: usize) -> alloc::vec::Vec<u8> {
        loop {
            let canonical = self.termios.lock().c_lflag & ICANON != 0;
            let bytes = {
                let mut input = self.input.lock();
                let bytes = input.read(len, canonical);
                if bytes.is_empty() {
                    unsafe { proc.lock.lock_manual() };
                    proc.sleep_channel = Some(self.input_channel());
                    proc.state = ProcState::Sleeping;
                }
                bytes
            };
            if !bytes.is_empty() {
                return bytes;
            }

            unsafe { proc.sleep_locked() };
        }
    }
}

impl FileOps for TTY {
    fn poll(&self, events: PollEvents) -> PollEvents {
        let mut ready = 0;
        // ppoll does not yet sleep on the TTY input channel, so report stdin
        // as readable and let read_blocking() provide the blocking behavior.
        if events & POLLIN != 0 {
            ready |= POLLIN;
        }

        // uart write never blocks so it's always avaliable
        if events & POLLOUT != 0 {
            ready |= POLLOUT;
        }
        ready
    }

    fn read(
        &self,
        proc: &mut super::Process,
        addr: usize,
        len: usize,
    ) -> Result<usize, super::fd::Errno> {
        if len == 0 {
            return Ok(0);
        }

        let bytes = self.read_blocking(proc, len);
        copy_out_cont(&mut proc.pagetable, addr, &bytes)
            .map(|()| bytes.len())
            .map_err(|()| Errno::EFAULT)
    }

    fn write(
        &self,
        proc: &mut super::Process,
        addr: usize,
        len: usize,
    ) -> Result<usize, super::fd::Errno> {
        let bytes = copy_in_cont(&mut proc.pagetable, addr, len).map_err(|()| Errno::EFAULT)?;
        self.transport.write(&bytes);
        Ok(bytes.len())
    }

    fn ioctl(&self, proc: &mut Process, op: usize, arg: usize) -> Result<usize, Errno> {
        const TCGETS: usize = 0x5401;
        const TCSETS: usize = 0x5402;
        const TIOCGPGRP: usize = 0x540f;
        const TIOCSPGRP: usize = 0x5410;
        const TIOCGWINSZ: usize = 0x5413;

        match op {
            TCGETS => {
                let termios = *self.termios.lock();
                copy_out(&mut proc.pagetable, arg, termios)
                    .map(|()| 0)
                    .map_err(|()| Errno::EFAULT)
            }
            TCSETS => {
                let termios =
                    copy_in::<Termios>(&mut proc.pagetable, arg).map_err(|()| Errno::EFAULT)?;
                *self.termios.lock() = termios;
                Ok(0)
            }
            TIOCGPGRP => {
                let pgid = (*self.foreground_pgid.lock()).unwrap_or(0) as u32;
                copy_out(&mut proc.pagetable, arg, pgid)
                    .map(|()| 0)
                    .map_err(|()| Errno::EFAULT)
            }
            TIOCSPGRP => {
                let pgid =
                    copy_in::<u32>(&mut proc.pagetable, arg).map_err(|()| Errno::EFAULT)? as usize;
                let kernel = KERNEL.get().unwrap().lock();
                if proc.sid.is_none()
                    || *self.session_id.lock() != proc.sid
                    || pgid == 0
                    || !kernel
                        .process_table
                        .iter()
                        .any(|process| process.pgid == Some(pgid) && process.sid == proc.sid)
                {
                    return Err(Errno::EPERM);
                }
                drop(kernel);
                *self.foreground_pgid.lock() = Some(pgid);
                Ok(0)
            }
            TIOCGWINSZ => {
                // struct winsize { unsigned short ws_row, ws_col, ws_xpixel, ws_ypixel; }
                let mut winsize = [0u8; 8];
                winsize[0..2].copy_from_slice(&24u16.to_ne_bytes());
                winsize[2..4].copy_from_slice(&80u16.to_ne_bytes());
                copy_out_cont(&mut proc.pagetable, arg, &winsize)
                    .map(|()| 0)
                    .map_err(|()| Errno::EFAULT)
            }
            _ => Err(Errno::ENOTTY),
        }
    }
}

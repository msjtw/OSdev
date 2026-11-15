use alloc::{
    collections::VecDeque,
    sync::Arc,
    vec::Vec,
};
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Once;

use crate::{
    KERNEL,
    errno::Errno,
    kernel::syscall::{SYS_READ, SYS_WRITE},
    lock::IntMutex,
    process::{
        ProcState, Process,
        fd::FileOps,
        ipc::{self, abi::Message},
        poll::{POLLIN, POLLOUT, PollEvents, wake_poll_waiters},
        signals::{self, SIGINT},
    },
    virtmemory::{copy_in, copy_in_cont, copy_out, copy_out_cont},
};

const ICRNL: u32 = 0o000400;
const ECHO: u32 = 0o000010;
const ICANON: u32 = 0o000002;
const ISIG: u32 = 0o000001;
const VINTR: usize = 0;

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
    uart: IntMutex<UartTransport>,
}

static TTY: Once<Arc<TTY>> = Once::new();
static INPUT_POLL_PENDING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Default)]
struct UartTransport {
    pid: Option<usize>,
    input_request: Option<usize>,
    echo_pending: Vec<u8>,
    echo_request: Option<usize>,
    echo_retry: Option<Vec<u8>>,
}

// Interrupt context only requests work. No allocation, IPC, or TTY locks here.
pub fn request_input_poll() {
    INPUT_POLL_PENDING.store(true, Ordering::Relaxed);
}

pub fn service_transport() {
    if let Some(tty) = TTY.get() {
        tty.poll_input();
        tty.poll_echo();
    }
}

impl TTY {
    fn new() -> Self {
        Self {
            termios: IntMutex::new(Termios::default()),
            session_id: IntMutex::new(None),
            foreground_pgid: IntMutex::new(None),
            input: IntMutex::new(TtyInput::default()),
            uart: IntMutex::new(UartTransport::default()),
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

    pub fn set_uart_pid(pid: usize) {
        Self::get().uart.lock().pid = Some(pid);
    }

    fn poll_input(&self) {
        let completion = {
            let mut state = self.uart.lock();
            let Some(pid) = state.pid else {
                return;
            };
            if let Some(id) = state.input_request {
                let mut kernel = KERNEL.get().unwrap().lock();
                let Some(reply) = ipc::kernel_read_reply(&mut kernel, pid, id) else {
                    return;
                };
                state.input_request = None;
                Some(reply)
            } else {
                None
            }
        };
        // Line discipline can acquire process/TTY locks and generate signals.
        // Do not run it while holding the kernel IPC or transport-state locks.
        if let Some((count, Some(bytes))) = completion {
            if count as isize >= 0 && count <= bytes.len() {
                if count != 0 {
                    request_input_poll();
                }
                for &byte in &bytes[..count] {
                    self.receive_byte(byte);
                }
            }
        }
        if !INPUT_POLL_PENDING.swap(false, Ordering::Relaxed) {
            return;
        }
        let mut state = self.uart.lock();
        let Some(pid) = state.pid else {
            return;
        };
        // A polling batch, not a UART protocol payload limit.
        let mut bytes = Vec::new();
        if bytes.try_reserve_exact(256).is_err() {
            request_input_poll();
            return;
        }
        bytes.resize(256, 0);
        let message = Message {
            operation: SYS_READ,
            args: [0, 0, bytes.len(), 0, 0, 0],
            ..Message::default()
        };
        let mut kernel = KERNEL.get().unwrap().lock();
        match ipc::kernel_send(&mut kernel, pid, message, Some(bytes)) {
            Ok(Some(id)) => state.input_request = Some(id),
            Ok(None) | Err(_) => request_input_poll(),
        }
    }

    fn poll_echo(&self) {
        let mut state = self.uart.lock();
        let Some(pid) = state.pid else {
            return;
        };
        let mut kernel = KERNEL.get().unwrap().lock();
        if let Some(id) = state.echo_request {
            let Some((result, data)) = ipc::kernel_read_reply(&mut kernel, pid, id) else {
                return;
            };
            state.echo_request = None;
            if let Some(mut bytes) = data {
                // Keep untransmitted echo in order across errors or short replies.
                if result as isize >= 0 && result <= bytes.len() {
                    bytes.drain(..result);
                }
                if !bytes.is_empty() {
                    state.echo_retry = Some(bytes);
                }
            }
        }
        if state.echo_retry.is_none() && !state.echo_pending.is_empty() {
            state.echo_retry = Some(core::mem::take(&mut state.echo_pending));
        }
        if state.echo_retry.is_some() {
            let message = Message {
                operation: SYS_WRITE,
                ..Message::default()
            };
            if let Ok(Some(id)) =
                ipc::kernel_send_retained(&mut kernel, pid, message, &mut state.echo_retry)
            {
                state.echo_request = Some(id);
            }
        }
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
            self.uart.lock().echo_pending.push(byte);
        }

        let canonical = termios.c_lflag & ICANON != 0;
        if input.readable(canonical) != 0 {
            // there is somethin to read so wakeup procees waiting for input
            KERNEL
                .get()
                .unwrap()
                .lock()
                .wakeup(Some(self.input_channel()), false);
            wake_poll_waiters();
        }
    }

    fn read_blocking(&self, proc: &mut Process, len: usize) -> Result<alloc::vec::Vec<u8>, Errno> {
        loop {
            if signals::has_unblocked_pending(proc) {
                return Err(Errno::EINTR);
            }
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
                return Ok(bytes);
            }

            unsafe { proc.sleep_locked() };
        }
    }
}

impl FileOps for TTY {
    fn poll(&self, events: PollEvents) -> PollEvents {
        let mut ready = 0;
        let canonical = self.termios.lock().c_lflag & ICANON != 0;
        if events & POLLIN != 0 && self.input.lock().readable(canonical) != 0 {
            ready |= POLLIN;
        }

        // No bounded output queue: accepting a write does not promise completion
        // without sleeping for UART IPC (the same distinction as a blocking fd).
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
    ) -> Result<usize, Errno> {
        if len == 0 {
            return Ok(0);
        }

        let bytes = self.read_blocking(proc, len)?;
        copy_out_cont(&mut proc.pagetable, addr, &bytes)
            .map(|()| bytes.len())
            .map_err(|()| Errno::EFAULT)
    }

    fn write(
        &self,
        proc: &mut super::Process,
        addr: usize,
        len: usize,
    ) -> Result<usize, Errno> {
        if len == 0 {
            return Ok(0);
        }
        let bytes = copy_in_cont(&mut proc.pagetable, addr, len).map_err(|()| Errno::EFAULT)?;
        let pid = self.uart.lock().pid.ok_or(Errno::ENXIO)?;
        let message = Message {
            operation: SYS_WRITE,
            ..Message::default()
        };
        let raw = ipc::kernel_call(proc, pid, message, Some(bytes))?;
        let count = serverlib::error::decode(raw).map_err(|error| match error {
            serverlib::Error::ENOMEM => Errno::ENOMEM,
            serverlib::Error::EFAULT => Errno::EFAULT,
            serverlib::Error::EINVAL => Errno::EINVAL,
            serverlib::Error::EINTR => Errno::EINTR,
            serverlib::Error::ESRCH => Errno::ESRCH,
            serverlib::Error::EPERM => Errno::EPERM,
            serverlib::Error::ENOSYS => Errno::ENOSYS,
            _ => Errno::EIO,
        })?;
        if count > len {
            return Err(Errno::EIO);
        }
        Ok(count)
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
                let previous = {
                    let mut current = self.termios.lock();
                    let previous = *current;
                    *current = termios;
                    previous
                };
                if (previous.c_lflag ^ termios.c_lflag) & ICANON != 0 {
                    // Already-buffered bytes can become readable without any
                    // new UART input. Let sleepers recheck the new line policy.
                    KERNEL
                        .get()
                        .unwrap()
                        .lock()
                        .wakeup(Some(self.input_channel()), true);
                    wake_poll_waiters();
                }
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

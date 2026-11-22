use alloc::sync::Arc;
use spin::Once;

use crate::{
    KERNEL,
    errno::Errno,
    lock::IntMutex,
    process::{Process, fd::FileOps, ipc, poll::PollEvents, terminal},
};
use serverlib::{
    abi::{Message, SYS_READ, SYS_WRITE},
    tty,
};

// Shared terminal descriptor adapter; terminal policy lives in tty_driver.
#[derive(Debug)]
pub(crate) struct TTY {
    session_id: IntMutex<Option<usize>>,
}

static TTY: Once<Arc<TTY>> = Once::new();

impl TTY {
    pub fn get() -> Arc<Self> {
        TTY.call_once(|| {
            Arc::new(Self {
                session_id: IntMutex::new(None),
            })
        });
        Arc::clone(TTY.get().unwrap())
    }

    /// Record the boot controlling session for the /dev/tty open check.
    pub fn set_controlling_session(sid: usize) {
        *Self::get().session_id.lock() = Some(sid);
    }

    pub fn is_controlling_terminal(proc: &Process) -> bool {
        proc.sid.is_some() && proc.sid == *Self::get().session_id.lock()
    }
}

// Forward an application operation without replacing its stamped identity.
fn call(
    proc: &mut Process,
    operation: usize,
    addr: usize,
    len: usize,
    ioctl: usize,
) -> Result<usize, Errno> {
    let pid = KERNEL.get().unwrap().lock().tty_pid.ok_or(Errno::ENXIO)?;
    let raw = ipc::user_call(
        proc,
        pid,
        Message {
            operation,
            args: [0, addr, len, ioctl, 0, 0],
            ..Message::default()
        },
    )?;
    serverlib::error::decode(raw).map_err(|error| match error.code() {
        1 => Errno::EPERM,
        3 => Errno::ESRCH,
        4 => Errno::EINTR,
        11 => Errno::EAGAIN,
        12 => Errno::ENOMEM,
        14 => Errno::EFAULT,
        22 => Errno::EINVAL,
        25 => Errno::ENOTTY,
        38 => Errno::ENOSYS,
        _ => Errno::EIO,
    })
}

impl FileOps for TTY {
    fn poll(&self, events: PollEvents) -> PollEvents {
        terminal::readiness(events)
    }

    fn read(&self, proc: &mut Process, addr: usize, len: usize) -> Result<usize, Errno> {
        if len == 0 {
            return Ok(0);
        }
        let count = call(proc, SYS_READ, addr, len, 0)?;
        if count > len {
            return Err(Errno::EIO);
        }
        Ok(count)
    }

    fn write(&self, proc: &mut Process, addr: usize, len: usize) -> Result<usize, Errno> {
        if len == 0 {
            return Ok(0);
        }
        let count = call(proc, SYS_WRITE, addr, len, 0)?;
        if count > len {
            return Err(Errno::EIO);
        }
        Ok(count)
    }

    fn ioctl(&self, proc: &mut Process, op: usize, arg: usize) -> Result<usize, Errno> {
        let len = match op {
            tty::TCGETS | tty::TCSETS => tty::Termios::SIZE,
            tty::TIOCGPGRP | tty::TIOCSPGRP => 4,
            tty::TIOCGWINSZ => 8,
            _ => return Err(Errno::ENOTTY),
        };
        call(proc, tty::IOCTL, arg, len, op)
    }
}

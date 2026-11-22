use crate::{Result, abi, error::decode, syscall::syscall};

/// Publish the configured terminal's readable level without consuming any input.
pub fn publish_readiness(readable: bool) -> Result<()> {
    decode(unsafe {
        syscall(
            abi::SYS_TTY_PUBLISH_READINESS,
            [usize::from(readable), 0, 0, 0, 0, 0],
        )
    })
    .map(|_| ())
}

/// Query the current SID behind a live request delivered to the configured TTY.
pub fn caller_session(id: usize) -> Result<usize> {
    decode(unsafe { syscall(abi::SYS_TTY_CALLER_SESSION, [id, 0, 0, 0, 0, 0]) })
}

/// Validate a foreground assignment without modifying the server's terminal state.
pub fn validate_foreground(id: usize, sid: usize, pgid: usize) -> Result<()> {
    decode(unsafe { syscall(abi::SYS_TTY_VALIDATE_FOREGROUND, [id, sid, pgid, 0, 0, 0]) })
        .map(|_| ())
}

/// Deliver terminal-originated SIGINT using kernel-owned process membership facts.
pub fn signal_group(sid: usize, pgid: usize) -> Result<()> {
    decode(unsafe { syscall(abi::SYS_TTY_SIGNAL_GROUP, [sid, pgid, 0, 0, 0, 0]) }).map(|_| ())
}

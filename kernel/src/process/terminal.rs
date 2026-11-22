use crate::process::poll::{self, POLLIN, POLLOUT, PollEvents};
use crate::{
    errno::Errno,
    kernel::Kernel,
    process::{ProcState, Process, ipc, signals},
};
use core::sync::atomic::{AtomicBool, Ordering};

static READABLE: AtomicBool = AtomicBool::new(false);

// Publish a TTY-owned readable level and wake existing poll waiters only on changes.
pub fn publish_readiness(
    kernel: &mut Kernel,
    proc: &Process,
    value: usize,
) -> Result<usize, Errno> {
    authorize(kernel, proc)?;
    let readable = match value {
        0 => false,
        1 => true,
        _ => return Err(Errno::EINVAL),
    };
    if READABLE.swap(readable, Ordering::AcqRel) != readable {
        // The syscall already holds KERNEL; wake_poll_waiters would reacquire it.
        kernel.wakeup(Some(poll::poll_channel()), true);
    }
    Ok(0)
}

// Read the published snapshot without locking, blocking or consuming terminal input.
pub fn readiness(events: PollEvents) -> PollEvents {
    (events & POLLOUT)
        | if READABLE.load(Ordering::Acquire) {
            events & POLLIN
        } else {
            0
        }
}

// Authorize only the actual TTY process configured at boot.
fn authorize(kernel: &Kernel, proc: &Process) -> Result<(), Errno> {
    if proc.pid.is_some() && kernel.tty_pid == proc.pid {
        Ok(())
    } else {
        Err(Errno::EPERM)
    }
}

// Resolve session identity through an unfinished delivered application request.
pub fn caller_session(kernel: &mut Kernel, proc: &Process, id: usize) -> Result<usize, Errno> {
    authorize(kernel, proc)?;
    Ok(ipc::caller_session(kernel, proc, id)?.unwrap_or(0))
}

// Preserve existing TIOCSPGRP checks without storing terminal policy in the kernel.
pub fn validate_foreground(
    kernel: &mut Kernel,
    proc: &Process,
    id: usize,
    sid: usize,
    pgid: usize,
) -> Result<usize, Errno> {
    let caller_sid = caller_session(kernel, proc, id)?;
    if caller_sid == 0
        || sid != caller_sid
        || pgid == 0
        || !kernel
            .process_table
            .iter()
            .any(|p| p.sid == Some(sid) && p.pgid == Some(pgid))
    {
        return Err(Errno::EPERM);
    }
    Ok(0)
}

// Queue terminal-originated SIGINT for live members of the supplied session/group.
pub fn signal_group(
    kernel: &mut Kernel,
    proc: &Process,
    sid: usize,
    pgid: usize,
) -> Result<usize, Errno> {
    authorize(kernel, proc)?;
    if sid == 0 || pgid == 0 {
        return Err(Errno::EPERM);
    }
    let bit = signals::signal_bit(signals::SIGINT).unwrap();
    for target in &mut kernel.process_table {
        let _guard = target.lock.lock();
        if target.sid == Some(sid)
            && target.pgid == Some(pgid)
            && matches!(
                target.state,
                ProcState::Used | ProcState::Sleeping | ProcState::Runnable | ProcState::Running
            )
        {
            target.pending_signals |= bit;
            if target.state == ProcState::Sleeping {
                target.state = ProcState::Runnable;
            }
        }
    }
    Ok(0)
}

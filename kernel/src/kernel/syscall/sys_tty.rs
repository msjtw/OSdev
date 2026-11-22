use crate::{
    KERNEL,
    process::{Process, terminal},
};

// Publish the terminal's readable level.
pub fn sys_tty_publish_readiness(proc: &mut Process) {
    let mut kernel = KERNEL.get().unwrap().lock();
    proc.trapframe.a0 = match terminal::publish_readiness(&mut kernel, proc, proc.trapframe.a0) {
        Ok(value) => value,
        Err(error) => error.into(),
    };
}

// Look up the session behind a live TTY request.
pub fn sys_tty_caller_session(proc: &mut Process) {
    let mut kernel = KERNEL.get().unwrap().lock();
    proc.trapframe.a0 = match terminal::caller_session(&mut kernel, proc, proc.trapframe.a0) {
        Ok(value) => value,
        Err(error) => error.into(),
    };
}

// Validate a requested foreground process group.
pub fn sys_tty_validate_foreground(proc: &mut Process) {
    let mut kernel = KERNEL.get().unwrap().lock();
    proc.trapframe.a0 = match terminal::validate_foreground(
        &mut kernel,
        proc,
        proc.trapframe.a0,
        proc.trapframe.a1,
        proc.trapframe.a2,
    ) {
        Ok(value) => value,
        Err(error) => error.into(),
    };
}

// Deliver terminal SIGINT to a process group.
pub fn sys_tty_signal_group(proc: &mut Process) {
    let mut kernel = KERNEL.get().unwrap().lock();
    proc.trapframe.a0 = match terminal::signal_group(
        &mut kernel,
        proc,
        proc.trapframe.a0,
        proc.trapframe.a1,
    ) {
        Ok(value) => value,
        Err(error) => error.into(),
    };
}

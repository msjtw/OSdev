use crate::{process::Process, virtmemory::copy_in_str};

pub fn sys_fork(proc: &mut Process) {
    let mut kernel = crate::KERNEL.get().unwrap().lock();
    proc.kfork(&mut kernel).unwrap();
}

pub fn sys_exec(proc: &mut Process) {
    let path_addr = proc.trapframe.a0;
    let argv_addr = proc.trapframe.a1;

    // let path = copy_in_str(proc.pagetable, path_addr);
}

pub fn sys_exit() {}

pub fn sys_getpid() {}

pub fn sys_wait() {}

pub fn sys_sbrk() {}

pub fn sys_pause() {}

pub fn sys_kill() {}

pub fn sys_uptime() {}


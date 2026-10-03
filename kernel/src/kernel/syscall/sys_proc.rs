use alloc::{string::String, vec::Vec};

use crate::{
    debug, println,
    process::Process,
    virtmemory::{copy_in, copy_in_str},
};

pub fn sys_fork(proc: &mut Process) {
    debug!("fork");
    proc.kfork().unwrap();
}

pub fn sys_exec(proc: &mut Process) {
    debug!("exec");
    let path_addr = proc.trapframe.a0;
    let mut argv_addr = proc.trapframe.a1;

    let path = copy_in_str(&mut proc.pagetable, path_addr).unwrap();

    let mut argv = Vec::<String>::new();
    loop {
        let arg_addr = copy_in::<usize>(&mut proc.pagetable, argv_addr).unwrap();
        if arg_addr == 0 {
            break;
        }

        let arg_str = copy_in_str(&mut proc.pagetable, arg_addr).unwrap();
        argv.push(arg_str);

        argv_addr += size_of::<usize>();
    }

    let argv_str = argv.iter().map(|s| s.as_ref()).collect();
    proc.kexec(path, argv_str).unwrap();
}

pub fn sys_wait(proc: &mut Process) {
    debug!("wait");
    let status_addr = proc.trapframe.a0;
    let ret = proc.kwait(status_addr);
    proc.trapframe.a0 = ret as usize;
}

pub fn sys_exit(proc: &mut Process) {
    debug!("exit");
    let status_addr = proc.trapframe.a0;
    let xstatus = proc.trapframe.a0;
    proc.kexit(xstatus as u32);
}

pub fn sys_getpid() {}

pub fn sys_gettid(proc: &mut Process) {
    proc.trapframe.a0 = proc.pid.unwrap_or(0);
}

pub fn sys_sbrk() {}

pub fn sys_pause() {}

pub fn sys_kill() {}

pub fn sys_uptime() {}

pub fn sys_mmap(proc: &mut Process) {
    println!(
        "mmap: addr={:#x} len={:#x} prot={:#x} flags={:#x} fd={} off={:#x}",
        proc.trapframe.a0,
        proc.trapframe.a1,
        proc.trapframe.a2,
        proc.trapframe.a3,
        proc.trapframe.a4,
        proc.trapframe.a5,
    );
    panic!("mmap");
}

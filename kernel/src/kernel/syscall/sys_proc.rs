use alloc::{string::String, vec::Vec};

use crate::{
    log,
    process::{ForkError, Process, fd::Errno},
    virtmemory::{copy_in, copy_in_str, copy_out_cont},
};

pub fn sys_clone(proc: &mut Process) {
    let flags = proc.trapframe.a0;
    let stack = proc.trapframe.a1;
    let parent_tid = proc.trapframe.a2;
    let tls = proc.trapframe.a3;
    let child_tid = proc.trapframe.a4;

    match proc.kfork(flags, stack, parent_tid, tls, child_tid) {
        Ok(pid) => proc.trapframe.a0 = pid,
        Err(ForkError::NoProcess) => proc.trapframe.a0 = Errno::EAGAIN.into(),
        Err(ForkError::Fault) => proc.trapframe.a0 = Errno::EFAULT.into(),
        Err(ForkError::InvalidArgument) => proc.trapframe.a0 = Errno::EINVAL.into(),
    }
}

pub fn sys_exec(proc: &mut Process) {
    log::debug!("exec");
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
    proc.trapframe.a0 = if proc.kexec(path, argv_str).is_err() {
        Errno::ENOENT.into()
    } else {
        0
    };
}

pub fn sys_waitid(proc: &mut Process) {
    const P_ALL: usize = 0;
    const WNOHANG: usize = 1;
    const WEXITED: usize = 4;
    const SIGCHLD: i32 = 17;
    const CLD_EXITED: i32 = 1;

    let idtype = proc.trapframe.a0;
    let id = proc.trapframe.a1;
    let info_addr = proc.trapframe.a2;
    let options = proc.trapframe.a3;
    let usage_addr = proc.trapframe.a4;

    if idtype != P_ALL
        || options & WEXITED == 0
        || options & !(WEXITED | WNOHANG) != 0
        || usage_addr != 0
    {
        log::logln!(
            "waitid: unsupported options idtype={} id={} options={:#x} usage={:#x}",
            idtype,
            id,
            options,
            usage_addr
        );
        proc.trapframe.a0 = Errno::EINVAL.into();
        return;
    }
    if info_addr == 0 {
        proc.trapframe.a0 = Errno::EFAULT.into();
        return;
    }

    let mut info = [0u8; 128];
    if copy_out_cont(&mut proc.pagetable, info_addr, &info).is_err() {
        proc.trapframe.a0 = Errno::EFAULT.into();
        return;
    }

    match proc.kwait(options & WNOHANG != 0) {
        Ok(Some((pid, status))) => {
            info[0..4].copy_from_slice(&SIGCHLD.to_ne_bytes());
            info[8..12].copy_from_slice(&CLD_EXITED.to_ne_bytes());
            info[12..16].copy_from_slice(&(pid as u32).to_ne_bytes());
            info[20..24].copy_from_slice(&(status as i32).to_ne_bytes());
            proc.trapframe.a0 = if copy_out_cont(&mut proc.pagetable, info_addr, &info).is_ok() {
                0
            } else {
                Errno::EFAULT.into()
            };
        }
        Ok(None) => proc.trapframe.a0 = 0,
        Err(()) => proc.trapframe.a0 = Errno::ECHILD.into(),
    }
}

pub fn sys_exit(proc: &mut Process) {
    log::debug!("exit");
    let xstatus = proc.trapframe.a0;
    proc.kexit(xstatus as u32);
}

pub fn sys_gettid(proc: &mut Process) {
    proc.trapframe.a0 = proc.pid.expect("Process with no pid");
}

pub fn sys_rt_sigprocmask(proc: &mut Process) {
    proc.trapframe.a0 = 0;
}

pub fn sys_set_tid_address(proc: &mut Process) {
    proc.trapframe.a0 = proc.pid.expect("Process with no pid");
}

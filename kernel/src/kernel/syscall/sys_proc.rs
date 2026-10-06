use alloc::{string::String, vec::Vec};

use crate::{
    log,
    process::{ForkError, Process},
    virtmemory::{copy_in, copy_in_str, copy_out_cont},
};

/// Support the fork-like Linux RISC-V clone form used by musl. Linux RISC-V
/// argument order is flags, stack, parent_tid, tls, child_tid. TLS and
/// child_tid are ignored unless their corresponding clone flags are set.
pub fn sys_clone(proc: &mut Process) {
    const SIGCHLD: usize = 17;
    const CSIGNAL: usize = 0xff;
    const CLONE_FILES: usize = 0x0000_0400;
    const CLONE_VFORK: usize = 0x0000_4000;
    const CLONE_PARENT_SETTID: usize = 0x0010_0000;
    const CLONE_CHILD_SETTID: usize = 0x0100_0000;

    let flags = proc.trapframe.a0;
    let stack = proc.trapframe.a1;
    let parent_tid = proc.trapframe.a2;
    let tls = proc.trapframe.a3;
    let child_tid = proc.trapframe.a4;

    let signal = flags & CSIGNAL;
    let clone_flags = flags & !CSIGNAL;

    if signal != SIGCHLD
        || clone_flags & !(CLONE_FILES | CLONE_VFORK | CLONE_PARENT_SETTID | CLONE_CHILD_SETTID) != 0
        || stack != 0
        || (parent_tid != 0 && clone_flags & CLONE_PARENT_SETTID == 0)
    {
        log::logln!(
            "clone: unsupported options flags={:#x} stack={:#x} parent_tid={:#x} tls={:#x} child_tid={:#x}",
            flags, stack, parent_tid, tls, child_tid
        );
        proc.trapframe.a0 = (-22isize) as usize; // EINVAL
        return;
    }

    let share_files = clone_flags & CLONE_FILES != 0;
    let vfork = clone_flags & CLONE_VFORK != 0;
    let parent_tid = (clone_flags & CLONE_PARENT_SETTID != 0).then_some(parent_tid);
    let child_tid = (clone_flags & CLONE_CHILD_SETTID != 0).then_some(child_tid);

    match proc.kfork(share_files, vfork, parent_tid, child_tid) {
        Ok(pid) => {
            proc.trapframe.a0 = pid;

            if vfork {
                // kfork published the sleeping state while holding the
                // process lock, before it made the child runnable.
                unsafe { proc.sleep_locked() };
            }
        }
        Err(ForkError::NoProcess) => proc.trapframe.a0 = (-11isize) as usize, // EAGAIN
        Err(ForkError::Fault) => proc.trapframe.a0 = (-14isize) as usize, // EFAULT
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
    if proc.kexec(path, argv_str).is_err() {
        proc.trapframe.a0 = (-2isize) as usize; // ENOENT (or exec setup failure)
    } else {
        proc.release_vfork_parent();
    }
}

/// Minimal Linux RISC-V waitid support: P_ALL + WEXITED, optionally WNOHANG.
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
            idtype, id, options, usage_addr
        );
        proc.trapframe.a0 = (-22isize) as usize; // EINVAL
        return;
    }
    if info_addr == 0 {
        proc.trapframe.a0 = (-14isize) as usize; // EFAULT
        return;
    }

    // Linux siginfo_t is 128 bytes on this 32-bit RISC-V ABI. Clear it first
    // both to validate the user pointer and to implement WNOHANG's no-result case.
    let mut info = [0u8; 128];
    if copy_out_cont(&mut proc.pagetable, info_addr, &info).is_err() {
        proc.trapframe.a0 = (-14isize) as usize; // EFAULT
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
                (-14isize) as usize // EFAULT
            };
        }
        Ok(None) => proc.trapframe.a0 = 0,
        Err(()) => proc.trapframe.a0 = (-10isize) as usize, // ECHILD
    }
}

pub fn sys_exit(proc: &mut Process) {
    log::debug!("exit");
    let xstatus = proc.trapframe.a0;
    proc.kexit(xstatus as u32);
}

pub fn sys_gettid(proc: &mut Process) {
    proc.trapframe.a0 = proc.pid.unwrap_or(0);
}

/// Signals are not implemented yet; accept mask changes as a compatibility stub.
pub fn sys_rt_sigprocmask(proc: &mut Process) {
    proc.trapframe.a0 = 0;
}

/// The kernel has no threads or futexes, so clear-child-TID handling is not
/// needed. Return the caller's TID as Linux requires.
pub fn sys_set_tid_address(proc: &mut Process) {
    proc.trapframe.a0 = proc.pid.unwrap_or(0);
}

pub fn sys_mmap(proc: &mut Process) {
    log::logln!(
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

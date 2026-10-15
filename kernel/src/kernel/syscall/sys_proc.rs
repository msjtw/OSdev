use alloc::{string::String, vec::Vec};

use crate::{
    KERNEL, log,
    process::{
        ForkError, Process,
        fd::Errno,
    },
    virtmemory::{copy_in, copy_in_str, copy_out, copy_out_cont},
};

//  Returns the real user ID of the calling process
pub fn sys_get_root_identity(proc: &mut Process) {
    proc.trapframe.a0 = 0;
}

// Returns  the  process ID (PID) of the calling process.
pub fn sys_getpid(proc: &mut Process) {
    proc.trapframe.a0 = proc.pid.unwrap_or(0);
}

// Return the parent process ID of the calling process.
pub fn sys_getppid(proc: &mut Process) {
    proc.trapframe.a0 = proc.parent.unwrap_or(0);
}

// setpgid()  sets  the PGID of the process specified by pid to pgid.
pub fn sys_setpgid(proc: &mut Process) {
    let caller_pid = proc.pid.expect("process without pid");

    // If pid is zero, then the process ID of the calling process is used.
    let target_pid = if proc.trapframe.a0 == 0 {
        caller_pid
    } else {
        proc.trapframe.a0
    };

    // If pgid is zero, then the  PGID  of  the process  specified
    // by pid is made the same as its process ID.
    let requested_pgid = if proc.trapframe.a1 == 0 {
        target_pid
    } else {
        proc.trapframe.a1
    };

    let mut kernel = KERNEL.get().unwrap().lock();
    let Some(target_index) = kernel
        .process_table
        .iter()
        .position(|process| process.pid == Some(target_pid))
    else {
        proc.trapframe.a0 = Errno::ESRCH.into();
        return;
    };

    let target = &kernel.process_table[target_index];
    if target_pid != caller_pid && target.parent != Some(caller_pid) {
        proc.trapframe.a0 = Errno::EPERM.into();
        return;
    }
    if requested_pgid != target_pid
        && !kernel
            .process_table
            .iter()
            .any(|process| process.pgid == Some(requested_pgid))
    {
        proc.trapframe.a0 = Errno::EPERM.into();
        return;
    }

    kernel.process_table[target_index].pgid = Some(requested_pgid);
    proc.trapframe.a0 = 0;
}

// getpgid() returns the PGID of the process specified by pid.
pub fn sys_getpgid(proc: &mut Process) {
    // If pid is zero, the process ID of the calling process is used.
    let target_pid = if proc.trapframe.a0 == 0 {
        proc.pid.expect("process without pid")
    } else {
        proc.trapframe.a0
    };

    proc.trapframe.a0 = match KERNEL
        .get()
        .unwrap()
        .lock()
        .process_table
        .iter()
        .find(|process| process.pid == Some(target_pid))
        .and_then(|process| process.pgid)
    {
        Some(pgid) => pgid,
        None => Errno::ESRCH.into(),
    };
}

// setsid()  creates  a  new  session if the calling process is not a process group leader.
// The calling process is the leader of the new session.
pub fn sys_setsid(proc: &mut Process) {
    let pid = proc.pid.expect("process without pid");
    {
        let kernel = KERNEL.get().unwrap().lock();
        if kernel
            .process_table
            .iter()
            .any(|process| process.pgid == Some(pid))
        {
            proc.trapframe.a0 = Errno::EPERM.into();
            return;
        }
    }

    proc.sid = Some(pid);
    proc.pgid = Some(pid);
    proc.trapframe.a0 = pid;
}

// Return a null-terminated string containing an absolute pathname
// that is the current working directory of the calling process.
pub fn sys_getcwd(proc: &mut Process) {
    let addr = proc.trapframe.a0;
    let size = proc.trapframe.a1;
    let mut bytes = proc
        .cwd
        .as_ref()
        .map_or(b"/".to_vec(), |cwd| cwd.as_bytes().to_vec());
    bytes.push(0);

    if size < bytes.len() {
        proc.trapframe.a0 = Errno::ERANGE.into();
        return;
    }
    proc.trapframe.a0 = match copy_out_cont(&mut proc.pagetable, addr, &bytes) {
        Ok(()) => addr,
        Err(()) => Errno::EFAULT.into(),
    };
}

// returns  system  information  in  the structure pointed to by buf.
pub fn sys_uname(proc: &mut Process) {
    const FIELD_LEN: usize = 65;
    #[repr(C)]
    struct Utsname {
        sysname: [u8; FIELD_LEN],
        nodename: [u8; FIELD_LEN],
        release: [u8; FIELD_LEN],
        version: [u8; FIELD_LEN],
        machine: [u8; FIELD_LEN],
        domainname: [u8; FIELD_LEN],
    }

    fn pad(value: &[u8]) -> [u8; FIELD_LEN] {
        let mut val: Vec<u8> = Vec::from(value);
        val.resize(FIELD_LEN, 0);
        val.try_into().unwrap_or([0; FIELD_LEN])
    }

    let uts = Utsname {
        sysname: pad(b"carlos"),
        nodename: pad(b"carlos"),
        release: pad(b"0"),
        version: pad(b"1"),
        machine: pad(b"qemu-riscv32"),
        domainname: pad(b"localdomain"),
    };

    proc.trapframe.a0 = match copy_out(&mut proc.pagetable, proc.trapframe.a0, uts) {
        Ok(()) => 0,
        Err(()) => Errno::EFAULT.into(),
    };
}

fn set_root_identity(proc: &mut Process) {
    proc.trapframe.a0 = if proc.trapframe.a0 == 0 {
        0
    } else {
        Errno::EPERM.into()
    };
}

// setuid()  sets  the effective user ID of the calling process.
pub fn sys_setuid(proc: &mut Process) {
    set_root_identity(proc);
}

// setgid()  sets the effective group ID of the calling process.
pub fn sys_setgid(proc: &mut Process) {
    set_root_identity(proc);
}

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
    let result = (|| {
        let path = copy_in_str(&mut proc.pagetable, proc.trapframe.a0).map_err(|_| Errno::EFAULT)?;
        let mut argv_addr = proc.trapframe.a1;

        let mut argv = Vec::<String>::new();
        if argv_addr != 0 {
            loop {
                let arg_addr = copy_in::<usize>(&mut proc.pagetable, argv_addr).map_err(|_| Errno::EFAULT)?;
                if arg_addr == 0 {
                    break;
                }
                if argv.len() == 64 {
                    return Err(Errno::E2BIG);
                }
                let arg = copy_in_str(&mut proc.pagetable, arg_addr).map_err(|_| Errno::EFAULT)?;
                argv.try_reserve(1).map_err(|_| Errno::ENOMEM)?;
                argv.push(arg);
                argv_addr = argv_addr.checked_add(size_of::<usize>()).ok_or(Errno::EFAULT)?;
            }
        }
        let argv_str = argv.iter().map(|s| s.as_ref()).collect();
        proc.kexec(path, argv_str)
    })();
    match result {
        Ok(_) => proc.trapframe.a0 = 0,
        Err(error) => proc.trapframe.a0 = error.into(),
    };
}

pub fn sys_waitid(proc: &mut Process) {
    const P_ALL: usize = 0; // Wait for any child; id is ignored.

    const WNOHANG: usize = 1;
    const WSTOPPED: usize = 2; // Wait for children that have been stopped by delivery of a signal.
    const WEXITED: usize = 4; // Wait for children that have terminated.
    const SIGCHLD: i32 = 17;
    const CLD_EXITED: i32 = 1;
    const CLD_KILLED: i32 = 2;

    let idtype = proc.trapframe.a0;
    let id = proc.trapframe.a1;
    let infop_addr = proc.trapframe.a2;
    let options = proc.trapframe.a3;
    // The  raw  waitid() system call takes a fifth argument, of type struct rusage *.
    // If this argument is non-NULL, then it is used to return resource  usage  information
    // about  the child, in the same manner as wait4(2).
    let rusage_addr = proc.trapframe.a4;

    if idtype != P_ALL
        || options & WEXITED == 0
        || options & !(WEXITED | WSTOPPED | WNOHANG) != 0
        || rusage_addr != 0
    {
        log::logln!(
            "waitid: unsupported options idtype={} id={} options={:#x} usage={:#x}",
            idtype,
            id,
            options,
            rusage_addr
        );
        proc.trapframe.a0 = Errno::EINVAL.into();
        return;
    }
    if infop_addr == 0 {
        proc.trapframe.a0 = Errno::EFAULT.into();
        return;
    }

    #[repr(C)]
    #[derive(Debug, Default)]
    struct Siginfo {
        si_signo: i32,
        si_errno: i32,
        si_code: i32,
        si_pid: i32,
        si_uid: u32,
        si_status: i32,
        _reserved: [u32; 26],
    }

    let info = Siginfo::default();
    if copy_out(&mut proc.pagetable, infop_addr, info).is_err() {
        proc.trapframe.a0 = Errno::EFAULT.into();
        return;
    }

    match proc.kwait(options & WNOHANG != 0) {
        Ok(Some((pid, status, signal))) => {
            let info = Siginfo {
                si_signo: SIGCHLD,
                si_errno: 0,
                si_code: if signal.is_some() { CLD_KILLED } else { CLD_EXITED },
                si_pid: pid as i32,
                si_uid: 0,
                si_status: signal.unwrap_or(status) as i32,
                _reserved: [0; 26],
            };
            proc.trapframe.a0 = match copy_out(&mut proc.pagetable, infop_addr, info) {
                Ok(_) => 0,
                Err(_) => Errno::EFAULT.into(),
            }
        }
        Ok(None) => proc.trapframe.a0 = 0,
        Err(()) => proc.trapframe.a0 = Errno::ECHILD.into(),
    }
}

pub fn sys_exit(proc: &mut Process) {
    let xstatus = proc.trapframe.a0;
    proc.kexit(xstatus as u32);
}

pub fn sys_gettid(proc: &mut Process) {
    proc.trapframe.a0 = proc.pid.expect("Process with no pid");
}

pub fn sys_set_tid_address(proc: &mut Process) {
    proc.trapframe.a0 = proc.pid.expect("Process with no pid");
}

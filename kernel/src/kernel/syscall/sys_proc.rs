use alloc::{string::String, vec::Vec};

use crate::{
    log,
    process::{ForkError, Process, fd::{Errno, POLLIN, POLLOUT, POLLNVAL}},
    read_csr64,
    virtmemory::{
        PAGESIZE, PTE_R, PTE_W, PTE_X, VmError, copy_in, copy_in_str, copy_out, copy_out_cont,
    },
};

const CLOCK_REALTIME: usize = 0;
const CLOCK_MONOTONIC: usize = 1;
// QEMU virt exposes a 10 MHz RISC-V time counter. This should eventually be
// obtained from the device tree's timebase-frequency property.
const TIMEBASE_HZ: u64 = 10_000_000;

#[repr(C)]
#[derive(Clone, Copy)]
struct Timespec64 {
    tv_sec: i64,
    tv_nsec: i64,
}

/// Linux RV32 clock_gettime64(2). Until an RTC or wall-clock source exists,
/// CLOCK_REALTIME and CLOCK_MONOTONIC both report elapsed boot time.
pub fn sys_clock_gettime64(proc: &mut Process) {
    let clock_id = proc.trapframe.a0;
    let timespec_addr = proc.trapframe.a1;

    if clock_id != CLOCK_REALTIME && clock_id != CLOCK_MONOTONIC {
        proc.trapframe.a0 = Errno::EINVAL.into();
        return;
    }

    let ticks = unsafe { read_csr64!(time) };
    let timespec = Timespec64 {
        tv_sec: (ticks / TIMEBASE_HZ) as i64,
        tv_nsec: ((ticks % TIMEBASE_HZ) * 1_000_000_000 / TIMEBASE_HZ) as i64,
    };

    proc.trapframe.a0 = match copy_out(&mut proc.pagetable, timespec_addr, timespec) {
        Ok(()) => 0,
        Err(()) => Errno::EFAULT.into(),
    };
}

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

// The  sigaction()  system call is used to change the action taken by a process
// on receipt of a specific signal.
pub fn sys_rt_sigaction(proc: &mut Process) {
    const RV32_SIGSET_SIZE: usize = 8;
    const RV32_SIGACTION_SIZE: usize = 20;

    let _signum = proc.trapframe.a0;
    let _act_addr = proc.trapframe.a1;
    let oldact_addr = proc.trapframe.a2;
    let sigsetsize = proc.trapframe.a3;

    if sigsetsize != RV32_SIGSET_SIZE {
        proc.trapframe.a0 = Errno::EINVAL.into();
        return;
    }

    // Signal delivery is not implemented yet.  Report an empty previous
    // disposition and accept the requested one so BusyBox can initialize.
    proc.trapframe.a0 = if oldact_addr == 0 {
        0
    } else {
        match copy_out_cont(&mut proc.pagetable, oldact_addr, &[0; RV32_SIGACTION_SIZE]) {
            Ok(()) => 0,
            Err(()) => Errno::EFAULT.into(),
        }
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

// brk() sets the end of the data segment to the value specified by addr
pub fn sys_brk(proc: &mut Process) {
    let addr = proc.trapframe.a0;
    proc.trapframe.a0 = proc.pagetable.brk(addr);
}

//  mmap() creates a new mapping in the virtual address space of the calling process.
pub fn sys_mmap(proc: &mut Process) {
    const PROT_READ: usize = 0x1;
    const PROT_WRITE: usize = 0x2;
    const PROT_EXEC: usize = 0x4;
    const MAP_PRIVATE: usize = 0x02;
    const MAP_FIXED: usize = 0x10;
    const MAP_ANONYMOUS: usize = 0x20;

    let addr = proc.trapframe.a0;
    let length = proc.trapframe.a1;
    let prot = proc.trapframe.a2;
    let flags = proc.trapframe.a3;
    let fd = proc.trapframe.a4;
    let offset = proc.trapframe.a5;

    if length == 0
        || flags & (MAP_PRIVATE | MAP_ANONYMOUS) != (MAP_PRIVATE | MAP_ANONYMOUS)
        || fd != usize::MAX
        || offset != 0
        || prot & !(PROT_READ | PROT_WRITE | PROT_EXEC) != 0
    {
        proc.trapframe.a0 = Errno::EINVAL.into();
        return;
    }

    // Musl reserves a guard page at the current brk with an anonymous,
    // MAP_FIXED, PROT_NONE request. Sv32 cannot represent a user leaf PTE
    // with no R/W/X bits
    if flags & MAP_FIXED != 0 {
        if prot == 0 && addr.is_multiple_of(PAGESIZE) {
            proc.trapframe.a0 = addr;
        } else {
            proc.trapframe.a0 = Errno::EINVAL.into();
        }
        return;
    }
    let mut perm = 0;
    if prot & PROT_READ != 0 {
        perm |= PTE_R;
    }
    if prot & PROT_WRITE != 0 {
        perm |= PTE_R | PTE_W;
    }
    if prot & PROT_EXEC != 0 {
        perm |= PTE_X;
    }
    proc.trapframe.a0 = match proc.pagetable.map_anonymous(addr, length, perm) {
        Ok(addr) => addr,
        Err(VmError::Invalid) => Errno::EINVAL.into(),
        Err(VmError::NoMemory) => Errno::ENOMEM.into(),
    };
}

// The munmap() system call deletes the mappings for the specified  address  range,
// and causes  further references to addresses within the range
// to generate invalid memory references.
pub fn sys_munmap(proc: &mut Process) {
    let addr = proc.trapframe.a0;
    let length = proc.trapframe.a1;
    proc.trapframe.a0 = match proc.pagetable.unmap(addr, length) {
        Ok(()) => 0,
        Err(_) => Errno::EINVAL.into(),
    };
}

pub fn sys_ppoll(proc: &mut Process) {
    const MAX_POLL_FDS: usize = 64;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct PollFd {
        fd: i32,
        events: i16,
        revents: i16,
    }

    let fds_addr = proc.trapframe.a0;
    let nfds = proc.trapframe.a1;
    if nfds > MAX_POLL_FDS {
        proc.trapframe.a0 = Errno::EINVAL.into();
        return;
    }

    let mut ready = 0;
    for index in 0..nfds {
        let Some(addr) = fds_addr.checked_add(index * core::mem::size_of::<PollFd>()) else {
            proc.trapframe.a0 = Errno::EFAULT.into();
            return;
        };
        let mut pollfd = match copy_in::<PollFd>(&mut proc.pagetable, addr) {
            Ok(pollfd) => pollfd,
            Err(()) => {
                proc.trapframe.a0 = Errno::EFAULT.into();
                return;
            }
        };

        pollfd.revents = match usize::try_from(pollfd.fd)
            .ok()
            .and_then(|fd| proc.fds.get(fd).ok())
        {
            Some(descriptor) => {
                let mut requested = pollfd.events;
                if !descriptor.readable {
                    requested &= !POLLIN;
                }
                if !descriptor.writable {
                    requested &= !POLLOUT;
                }
                descriptor.target.poll(requested)
            }
            None => POLLNVAL,
        };

        if pollfd.revents != 0 {
            ready += 1;
        }
        let bytes = unsafe {
            core::slice::from_raw_parts(
                (&pollfd as *const PollFd).cast::<u8>(),
                core::mem::size_of::<PollFd>(),
            )
        };
        if copy_out_cont(&mut proc.pagetable, addr, bytes).is_err() {
            proc.trapframe.a0 = Errno::EFAULT.into();
            return;
        }
    }

    proc.trapframe.a0 = ready;
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
    match proc.kexec(path, argv_str) {
        Ok(_) => proc.trapframe.a0 = 0,
        Err(_) => proc.trapframe.a0 = Errno::ENOENT.into(),
    };
}

pub fn sys_waitid(proc: &mut Process) {
    const P_ALL: usize = 0; // Wait for any child; id is ignored.

    const WNOHANG: usize = 1;
    const WSTOPPED: usize = 2; // Wait for children that have been stopped by delivery of a signal.
    const WEXITED: usize = 4; // Wait for children that have terminated.
    const SIGCHLD: i32 = 17;
    const CLD_EXITED: i32 = 1;

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
        Ok(Some((pid, status))) => {
            let info = Siginfo {
                si_signo: SIGCHLD,
                si_errno: 0,
                si_code: CLD_EXITED,
                si_pid: pid as i32,
                si_uid: 0,
                si_status: status as i32,
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

mod sys_file;
mod sys_memory;
mod sys_proc;
mod sys_signals;
mod sys_shutdown;
mod sys_time;

use crate::{
    kernel::syscall::{
        sys_file::{
            sys_close, sys_dup, sys_dup3, sys_fcntl, sys_getdents64, sys_ioctl, sys_openat,
            sys_pipe2,
            sys_lseek, sys_ppoll, sys_read, sys_statx, sys_write, sys_writev,
        },
        sys_memory::{sys_brk, sys_mmap, sys_munmap},
        sys_proc::*,
        sys_signals::{sys_rt_sigaction, sys_rt_sigprocmask},
        sys_shutdown::{sys_kill, sys_reboot, sys_sync},
        sys_time::{sys_clock_getres_time64, sys_clock_gettime64, sys_clock_nanosleep_time64},
    },
    log,
    process::Process,
};

// Linux RISC-V syscall numbers.
pub const SYS_GETCWD: usize = 17;
pub const SYS_CHDIR: usize = 49;
pub const SYS_IOCTL: usize = 29;
pub const SYS_FCNTL: usize = 25;
pub const SYS_DUP: usize = 23;
pub const SYS_DUP3: usize = 24;
pub const SYS_CLOSE: usize = 57;
pub const SYS_OPENAT: usize = 56;
pub const SYS_GETDENTS64: usize = 61;
pub const SYS_LSEEK: usize = 62;
pub const SYS_PIPE2: usize = 59;
pub const SYS_READ: usize = 63;
pub const SYS_WRITE: usize = 64;
pub const SYS_WRITEV: usize = 66;
pub const SYS_EXIT: usize = 93;
pub const SYS_EXIT_GROUP: usize = 94;
pub const SYS_WAITID: usize = 95;
pub const SYS_SYNC: usize = 81;
pub const SYS_KILL: usize = 129;
pub const SYS_REBOOT: usize = 142;
pub const SYS_SET_TID_ADDRESS: usize = 96;
pub const SYS_RT_SIGPROCMASK: usize = 135;
pub const SYS_RT_SIGACTION: usize = 134;
pub const SYS_SETGID: usize = 144;
pub const SYS_SETUID: usize = 146;
pub const SYS_SETPGID: usize = 154;
pub const SYS_GETPGID: usize = 155;
pub const SYS_SETSID: usize = 157;
pub const SYS_GETUID: usize = 174;
pub const SYS_GETEUID: usize = 175;
pub const SYS_GETGID: usize = 176;
pub const SYS_GETEGID: usize = 177;
pub const SYS_GETTID: usize = 178;
pub const SYS_UNAME: usize = 160;
pub const SYS_GETPID: usize = 172;
pub const SYS_GETPPID: usize = 173;
pub const SYS_BRK: usize = 214;
pub const SYS_MUNMAP: usize = 215;
pub const SYS_CLONE: usize = 220;
pub const SYS_EXECVE: usize = 221;
pub const SYS_MMAP: usize = 222;
pub const SYS_STATX: usize = 291;
pub const SYS_CLOCK_GETTIME64: usize = 403;
pub const SYS_CLOCK_GETRES_TIME64: usize = 406;
pub const SYS_CLOCK_NANOSLEEP_TIME64: usize = 407;
pub const SYS_PPOLL_TIME64: usize = 414;

// NOTE:
// syscall number: a7
// arguments: a0-a5
// return value: a0
// all in user registers in trapframe

pub fn syscall(proc: &mut Process) {
    let sys_num = proc.trapframe.a7;
    log::debug!("syscall: {}", sys_num);
    // let args: [u32; 6];

    match sys_num {
        SYS_GETCWD => sys_getcwd(proc),
        SYS_CHDIR => sys_chdir(proc),
        SYS_SYNC => sys_sync(proc),
        SYS_KILL => sys_kill(proc),
        SYS_REBOOT => sys_reboot(proc),
        SYS_DUP => sys_dup(proc),
        SYS_DUP3 => sys_dup3(proc),
        SYS_CLOSE => sys_close(proc),
        SYS_OPENAT => sys_openat(proc),
        SYS_GETDENTS64 => sys_getdents64(proc),
        SYS_LSEEK => sys_lseek(proc),
        SYS_PIPE2 => sys_pipe2(proc),
        SYS_READ => sys_read(proc),
        SYS_WRITE => sys_write(proc),
        SYS_WRITEV => sys_writev(proc),
        SYS_IOCTL => sys_ioctl(proc),
        SYS_FCNTL => sys_fcntl(proc),
        SYS_RT_SIGACTION => sys_rt_sigaction(proc),
        SYS_RT_SIGPROCMASK => sys_rt_sigprocmask(proc),
        SYS_UNAME => sys_uname(proc),
        SYS_GETPID => sys_getpid(proc),
        SYS_GETPPID => sys_getppid(proc),
        SYS_SETGID => sys_setgid(proc),
        SYS_SETUID => sys_setuid(proc),
        SYS_SETPGID => sys_setpgid(proc),
        SYS_GETPGID => sys_getpgid(proc),
        SYS_SETSID => sys_setsid(proc),
        SYS_GETUID | SYS_GETEUID | SYS_GETGID | SYS_GETEGID => sys_get_root_identity(proc),
        SYS_CLONE => sys_clone(proc),
        SYS_EXECVE => sys_exec(proc),
        SYS_WAITID => sys_waitid(proc),
        SYS_SET_TID_ADDRESS => sys_set_tid_address(proc),
        SYS_EXIT => sys_exit(proc),
        SYS_EXIT_GROUP => sys_exit(proc),
        SYS_GETTID => sys_gettid(proc),
        SYS_BRK => sys_brk(proc),
        SYS_MMAP => sys_mmap(proc),
        SYS_MUNMAP => sys_munmap(proc),
        SYS_STATX => sys_statx(proc),
        SYS_CLOCK_GETTIME64 => sys_clock_gettime64(proc),
        SYS_CLOCK_GETRES_TIME64 => sys_clock_getres_time64(proc),
        SYS_CLOCK_NANOSLEEP_TIME64 => sys_clock_nanosleep_time64(proc),
        SYS_PPOLL_TIME64 => sys_ppoll(proc),
        _ => {
            log::logln!("unimplemented syscall: {}", sys_num);
            proc.trapframe.a0 = (-38isize) as usize;
        }
    }
}

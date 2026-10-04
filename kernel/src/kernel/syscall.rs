#![allow(dead_code)]

mod sys_file;
mod sys_proc;

use crate::{
    kernel::syscall::{
        sys_file::{sys_ioctl, sys_write, sys_writev},
        sys_proc::*,
    },
    print,
    process::Process,
};

// System call numbers
pub const SYS_FORK: usize = 1;
pub const SYS_EXIT: usize = 2;
pub const SYS_WAIT: usize = 3;
pub const SYS_PIPE: usize = 4;
pub const SYS_READ: usize = 5;
pub const SYS_KILL: usize = 6;
pub const SYS_EXEC: usize = 7;
pub const SYS_FSTAT: usize = 8;
pub const SYS_CHDIR: usize = 9;
pub const SYS_DUP: usize = 10;
pub const SYS_GETPID: usize = 11;
pub const SYS_SBRK: usize = 12;
pub const SYS_PAUSE: usize = 13;
pub const SYS_UPTIME: usize = 14;
pub const SYS_OPEN: usize = 15;
pub const SYS_WRITE: usize = 16;
pub const SYS_MKNOD: usize = 17;
pub const SYS_UNLINK: usize = 18;
pub const SYS_LINK: usize = 19;
pub const SYS_MKDIR: usize = 20;
pub const SYS_CLOSE: usize = 21;
// Linux RISC-V syscall number used by libc for terminal queries.
pub const SYS_IOCTL: usize = 29;
pub const SYS_WRITEV: usize = 66;

pub const SYS_GETTID: usize = 96;
pub const SYS_EXIT_GROUP: usize = 94;
pub const SYS_MMAP: usize = 222;

// NOTE:
// syscall number: a7
// arguments: a0-a5
// return value: a0
// all in user registers in trapframe

pub fn syscall(proc: &mut Process) {
    let sys_num = proc.trapframe.a7;
    // let args: [u32; 6];

    match sys_num {
        SYS_WRITE => sys_write(proc),
        SYS_WRITEV => sys_writev(proc),
        SYS_IOCTL => sys_ioctl(proc),
        SYS_FORK => sys_fork(proc),
        SYS_EXEC => sys_exec(proc),
        SYS_WAIT => sys_wait(proc),
        SYS_EXIT => sys_exit(proc),
        SYS_EXIT_GROUP => sys_exit(proc),
        SYS_GETTID => sys_gettid(proc),
        SYS_MMAP => sys_mmap(proc),
        // Return -ENOSYS for unsupported calls instead of crashing the kernel.
        _ => {
            print!("unimplemented syscall: {}\n", sys_num);
            proc.trapframe.a0 = (-38isize) as usize;
        }
    }
}

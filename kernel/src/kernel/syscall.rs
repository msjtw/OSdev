mod sys_file;
mod sys_proc;

use crate::{
    kernel::syscall::{
        sys_file::{sys_close, sys_dup, sys_dup3, sys_ioctl, sys_read, sys_write, sys_writev},
        sys_proc::*,
    },
    log,
    process::Process,
};

// Linux RISC-V syscall numbers.
pub const SYS_IOCTL: usize = 29;
pub const SYS_DUP: usize = 23;
pub const SYS_DUP3: usize = 24;
pub const SYS_CLOSE: usize = 57;
// pub const SYS_PIPE2: usize = 59;
pub const SYS_READ: usize = 63;
pub const SYS_WRITE: usize = 64;
pub const SYS_WRITEV: usize = 66;
pub const SYS_EXIT: usize = 93;
pub const SYS_EXIT_GROUP: usize = 94;
pub const SYS_WAITID: usize = 95;
pub const SYS_SET_TID_ADDRESS: usize = 96;
pub const SYS_RT_SIGPROCMASK: usize = 135;
pub const SYS_GETTID: usize = 178;
pub const SYS_CLONE: usize = 220;
pub const SYS_EXECVE: usize = 221;
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
        SYS_DUP => sys_dup(proc),
        SYS_DUP3 => sys_dup3(proc),
        SYS_CLOSE => sys_close(proc),
        // SYS_PIPE2 => sys_pipe2(proc),
        SYS_READ => sys_read(proc),
        SYS_WRITE => sys_write(proc),
        SYS_WRITEV => sys_writev(proc),
        SYS_IOCTL => sys_ioctl(proc),
        SYS_RT_SIGPROCMASK => sys_rt_sigprocmask(proc),
        SYS_CLONE => sys_clone(proc),
        SYS_EXECVE => sys_exec(proc),
        SYS_WAITID => sys_waitid(proc),
        SYS_SET_TID_ADDRESS => sys_set_tid_address(proc),
        SYS_EXIT => sys_exit(proc),
        SYS_EXIT_GROUP => sys_exit(proc),
        SYS_GETTID => sys_gettid(proc),
        SYS_MMAP => sys_mmap(proc),
        // Return -ENOSYS for unsupported calls instead of crashing the kernel.
        _ => {
            log::logln!("unimplemented syscall: {}", sys_num);
            proc.trapframe.a0 = (-38isize) as usize;
        }
    }
}

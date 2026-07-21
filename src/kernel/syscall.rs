#![allow(dead_code)]

use crate::{print, process::Process, uart_print, virtmemory::copy_in_bytes};
use alloc::{format, string::String};

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

// NOTE:
// syscall number: a7
// arguments: a0-a5
// return value: a0
// all in user registers in trapframe

pub fn syscall(proc: &mut Process) {
    let sys_num = proc.trapframe.a7 as usize;
    // let args: [u32; 6];

    // print!("call num: {sys_num}\n")

    match sys_num {
        SYS_WRITE => {
            // fd, addr, size
            sys_write(proc);
        }
        _ => {
            panic!("unimplemented syscall {sys_num}")
        }
    }
}

fn sys_write(proc: &mut Process) {
    unsafe {
        (*crate::CPU).push_interrupt_off();
    }
    let fd = proc.trapframe.a0 as usize;
    let addr = proc.trapframe.a1 as usize;
    let size = proc.trapframe.a2 as usize;

    if fd != 1 {
        panic!("Write to fd {fd}");
    }

    let bytes = copy_in_bytes(&proc.pagetable.unwrap(), addr, size).unwrap();
    let msg = String::from_utf8(bytes).unwrap();

    uart_print(&msg);
    proc.trapframe.a0 = 0;
    unsafe {
        (*crate::CPU).pop_interrupt_off();
    }
}

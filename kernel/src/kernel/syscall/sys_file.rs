use alloc::string::String;

use crate::{process::Process, uart_print, virtmemory::copy_in_cont};


pub fn sys_write(proc: &mut Process) {
    let fd = proc.trapframe.a0;
    let addr = proc.trapframe.a1;
    let size = proc.trapframe.a2;

    if fd != 1 {
        panic!("Write to fd {fd}");
    }

    let bytes = copy_in_cont(&mut proc.pagetable, addr, size).unwrap();
    let msg = String::from_utf8(bytes).unwrap();

    uart_print(&msg);
    proc.trapframe.a0 = 0;
}

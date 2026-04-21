#![no_std]
#![no_main]
#![feature(ascii_char)]

mod csr;
mod kernel;
mod process;
mod trap;
pub mod virtmemory;

extern crate alloc;
use alloc::boxed::Box;
use alloc::format;
use buddy_system_allocator::LockedHeap;

use core::arch::{asm, global_asm};
use core::panic::PanicInfo;
use core::ptr::write_volatile;

use crate::kernel::Kernel;
use crate::process::{Process, kexec};
use crate::trap::init_trap;
use crate::virtmemory::RAMEND;

#[global_allocator]
static HEAP_ALLOCATOR: LockedHeap<32> = LockedHeap::<32>::new();

// #[unsafe(no_mangle)]
// unsafe extern  static STACK: [u8; 4096] = [0; 4096];

global_asm!(
    "
    .global _entry
    .extern _STACK_PTR
    .extern stack

    .section .text.boot

    _entry:
        la sp, _STACK_PTR
        call main

    spin:
        j spin
    "
);
 
fn uart_print(message: &str) {
    let uart = virtmemory::UART as *mut u8;
    for c in message.bytes() {
        unsafe {
            write_volatile(uart, c);
        }
    }
}

#[allow(static_mut_refs)]
#[unsafe(no_mangle)]
pub extern "C" fn main() -> ! {
    uart_print("Hello, world!\n");

    // TODO: How to implement memory so all acceses dont have to be unsafe.
    //       Can I map a slice [u8] over whole avaliable ram?

    // Init physical memory allocator.
    unsafe {
        let ekernel = &virtmemory::ekernel as *const u32 as usize;
        HEAP_ALLOCATOR
            .lock()
            .init(ekernel, RAMEND as usize - ekernel);
    }

    let mut kernel = Box::new(Kernel::default());

    kernel.initproc(8);
    init_trap();
    kernel.kvm.start_kvm();

    uart_print("Virt started\n");
    let bytes = include_bytes!("../../user_proc/shell.bin");

    kexec(&mut kernel.process_table[0], bytes);

    process::scheduler(&mut kernel);
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    let msg = format!("Something went wrong. {:?}\n", info);
    uart_print(msg.as_str());
    loop {}
}

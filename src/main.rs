#![no_std]
#![no_main]
#![feature(ascii_char)]

mod csr;
mod process;
mod trap;
pub mod virtmemory;

extern crate alloc;
use alloc::{format, vec};
use buddy_system_allocator::LockedHeap;

use core::arch::{asm, global_asm};
use core::panic::PanicInfo;
use core::ptr::write_volatile;

use crate::virtmemory::RAMEND;
use crate::trap::init_trap;

const HEAPSIZE: usize = 0x10000;

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

struct Kernel {
    kvm: virtmemory::Kvm,
    // processes: [process::Process; 8]
}

#[allow(static_mut_refs)]
#[unsafe(no_mangle)]
pub extern "C" fn main() -> ! {
    uart_print("Hello, world!\n");

    unsafe {
        let ekernel = &virtmemory::ekernel as *const u32 as usize;
        HEAP_ALLOCATOR.lock().init(ekernel, RAMEND - ekernel);
    }

    let kvm = virtmemory::Kvm::init().unwrap();

    let kernel = Kernel { kvm };
    init_trap();

    // let msg = unsafe {
    //     format!(
    //         "etext: 0x{:x}, ekernel: 0x{:x}, _STACK_PTR: 0x{:x}\n",
    //         &kmemory::etext as *const u32 as u32,
    //         &kmemory::ekernel as *const u32 as u32,
    //         &kmemory::_STACK_PTR as *const u32 as u32
    //     )
    // };
    // let tmp = msg.as_str();
    // uart_print(tmp);

    // kernel.kvm.start_kvm();

    // uart_print("Virt started\n");

    unsafe { asm!("unimp") };
    loop {}
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    let msg = format!("Something went wrong. {:?}\n", info);
    uart_print(msg.as_str());
    loop {}
}

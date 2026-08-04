#![no_std]
#![no_main]
#![feature(allocator_api)]
#![allow(static_mut_refs)]

pub mod allocator;
mod csr;
mod kernel;
pub mod lock;
mod process;
mod trap;
pub mod virtmemory;

extern crate alloc;
use alloc::boxed::Box;
use alloc::{format, vec};
use spin::Once;

use core::arch::global_asm;
use core::panic::PanicInfo;
use core::ptr::write_volatile;

use crate::kernel::{Cpu, Kernel};
use crate::trap::init_trap;
use crate::trap::trampoline::{userret, uservec};
use crate::virtmemory::RAMEND;

const USER_BYTES: &[u8; 3449] = include_bytes!("../../user/_div.bin");

#[global_allocator]
static HEAP_ALLOCATOR: allocator::LockedHeap<32> = allocator::LockedHeap::<32>::new();

static FRAME_ALLOCATOR: allocator::FrameAllocator = allocator::FrameAllocator {};

static mut CPU: Cpu = Cpu::new();

static KERNEL: Once<lock::IntMutex<Kernel>> = Once::new();

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

#[macro_export]
macro_rules! print {
    () => {
        $crate::uart_print("")
    };
    ($($arg:tt)*) => {{
        $crate::uart_print(&format!($($arg)*));
    }};
}

pub fn uart_print(message: &str) {
    let uart = virtmemory::UART as *mut u8;
    for c in message.bytes() {
        unsafe {
            write_volatile(uart, c);
        }
    }
}

// FIX: Stack guard pages don't work,
// stack-overflow causes infinite trapping.

#[unsafe(no_mangle)]
pub extern "C" fn main() -> ! {
    // NOTE: without this they are optimized away
    let _ = uservec as *const () as usize;
    let _ = userret as *const () as usize;

    // TODO: How to implement memory so all accesses don't have to be unsafe.
    //       Can I map a slice [u8] over whole available ram?

    // Init physical memory allocator.
    unsafe {
        let ekernel = &virtmemory::ekernel as *const usize as usize;
        HEAP_ALLOCATOR
            .lock()
            .init(ekernel, RAMEND as usize - ekernel);
    }

    init_trap();
    KERNEL.call_once(|| lock::IntMutex::new(Kernel::default()));

    print!("Hello world\n");

    KERNEL
        .get()
        .unwrap()
        .lock()
        .init()
        .expect("Kernel init fail");

    KERNEL.get().unwrap().lock().initproc(4).unwrap();
    KERNEL
        .get()
        .unwrap()
        .lock()
        .kvm
        .as_mut()
        .expect("KVM not initialized")
        .start_kvm();
    print!("Virt started\n");

    // Start init
    {
        let mut kernel = KERNEL.get().unwrap().lock();

        let user_p0 = kernel.allocproc().unwrap();
        user_p0.kexec(USER_BYTES, vec!["10"]).unwrap();
        user_p0.state = process::ProcState::Runnable;

        let user_p1 = kernel.allocproc().unwrap();
        user_p1.kexec(USER_BYTES, vec!["17"]).unwrap();
        user_p1.state = process::ProcState::Runnable;
    }

    process::scheduler();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    print!("Something went wrong. {:?}\n", info);
    loop {}
}

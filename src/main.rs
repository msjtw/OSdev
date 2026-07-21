#![no_std]
#![no_main]
#![feature(allocator_api)]

pub mod allocator;
mod csr;
mod kernel;
mod process;
mod trap;
pub mod virtmemory;

extern crate alloc;
use alloc::boxed::Box;
use alloc::format;

use core::arch::global_asm;
use core::panic::PanicInfo;
use core::ptr::{null_mut, write_volatile};

use crate::kernel::{Cpu, Kernel};
use crate::trap::init_trap;
use crate::trap::trampoline::{userret, uservec};
use crate::virtmemory::RAMEND;

const USER_BYTES: &[u8; 3433] = include_bytes!("../../user/_div.bin");

#[global_allocator]
static HEAP_ALLOCATOR: allocator::LockedHeap<32> = allocator::LockedHeap::<32>::new();

static FRAME_ALLOCATOR: allocator::FrameAllocator = allocator::FrameAllocator {};

static mut CPU: *mut Cpu = null_mut();

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

#[unsafe(no_mangle)]
pub extern "C" fn main() -> ! {
    // NOTE: without this they are optimized away
    let _ = uservec as *const () as usize;
    let _ = userret as *const () as usize;

    // TODO: How to implement memory so all acceses dont have to be unsafe.
    //       Can I map a slice [u8] over whole avaliable ram?

    // NOTE: Temporary stack allocated cpu struct used to initialize memory.
    // Later replaced with heap allocated one in Kernel.
    let mut tmp_cpu = Cpu::default();
    unsafe {
        CPU = &raw mut tmp_cpu;
    }

    // Init physical memory allocator.
    unsafe {
        let ekernel = &virtmemory::ekernel as *const u32 as usize;
        HEAP_ALLOCATOR
            .lock()
            .init(ekernel, RAMEND as usize - ekernel);
    }

    init_trap();
    let mut kernel = Box::new(Kernel::default());
    unsafe {
        CPU = &raw mut kernel.cpus;
    }

    print!("Hello world\n");

    kernel.init().expect("Kernel init fail");

    kernel.initproc(4).unwrap();
    kernel
        .kvm
        .as_mut()
        .expect("KVM not initialized")
        .start_kvm();
    print!("Virt started\n");

    let user_p0 = kernel.allocproc().unwrap();
    user_p0.kexec(USER_BYTES).unwrap();
    let user_p1 = kernel.allocproc().unwrap();
    user_p1.kexec(USER_BYTES).unwrap();

    process::scheduler(kernel);
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    print!("Something went wrong. {:?}\n", info);
    loop {}
}

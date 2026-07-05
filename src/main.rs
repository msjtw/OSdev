#![no_std]
#![no_main]
#![feature(allocator_api)]

mod csr;
pub mod frame_allocator;
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
use crate::process::{Context, Cpu};
use crate::trap::trampoline::{userret, uservec};
use crate::trap::{init_trap, interrupt_off, interrupt_on, usertrap};
use crate::virtmemory::RAMEND;

const USER_BYTES: &[u8; 531684] = include_bytes!("../../shell/main.bin.o");

#[global_allocator]
static HEAP_ALLOCATOR: LockedHeap<32> = LockedHeap::<32>::new();

static FRAME_ALLOCATOR: frame_allocator::FrameAllocator = frame_allocator::FrameAllocator {};

// #[unsafe(no_mangle)]
// unsafe extern  static STACK: [u8; 4096] = [0; 4096];

static mut CPU: Cpu = Cpu {
    current: core::ptr::null_mut(),
    context: Context::zero(),
};

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
        $crate::uart_print(&format!($($arg)*))
    }};
}

fn uart_print(message: &str) {
    let uart = virtmemory::UART as *mut u8;
    for c in message.bytes() {
        unsafe {
            write_volatile(uart, c);
        }
    }
}

fn proc0() {
    let mut count = 0;
    unsafe {
        interrupt_on();
    }
    loop {
        unsafe {
            let sstatus = read_csr!(sstatus);
            print!("{:b}\n", sstatus);
        }
        for _ in 0..100_000 {
            unsafe { asm!("nop") };
        }
        print!("PROC 0 {count}\n");
        count += 2;
        unsafe {
            asm!("wfi");
            // (*CPU.current).yeld();
        };
    }
}

fn proc1() {
    let mut count = 1;
    unsafe {
        interrupt_on();
    }
    loop {
        unsafe {
            let sstatus = read_csr!(sstatus);
            print!("{:b}\n", sstatus);
        }
        for _ in 0..100_000 {
            unsafe { asm!("nop") };
        }
        print!("PROC 1 {count}\n");
        count += 2;
        unsafe {
            asm!("wfi");
            // (*CPU.current).yeld();
        };
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn main() -> ! {
    // NOTE: without this they are optimized away
    let _ = uservec as *const () as usize;
    let _ = userret as *const () as usize;

    // TODO: How to implement memory so all acceses dont have to be unsafe.
    //       Can I map a slice [u8] over whole avaliable ram?

    // Init physical memory allocator.
    unsafe {
        let ekernel = &virtmemory::ekernel as *const u32 as usize;
        HEAP_ALLOCATOR
            .lock()
            .init(ekernel, RAMEND as usize - ekernel);
    }

    print!("Hello world\n");

    init_trap();
    let mut kernel = Box::new(Kernel::default());

    kernel.initproc(8).unwrap();
    kernel.kvm.start_kvm();
    print!("Virt started\n");

    let user_p0 = kernel.allocproc(proc0).unwrap();
    user_p0.kexec(USER_BYTES).unwrap();
    let user_p1 = kernel.allocproc(proc1).unwrap();
    user_p1.kexec(USER_BYTES).unwrap();



    print!("processes\n");
    process::scheduler(kernel);

    // loop {
    //     unsafe { asm!("wfi") };
    // }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    print!("Something went wrong. {:?}\n", info);
    loop {}
}

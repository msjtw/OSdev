#![no_std]
#![no_main]
#![feature(allocator_api)]
#![allow(static_mut_refs)]

pub mod allocator;
mod csr;
mod drivers;
mod kernel;
pub mod lock;
mod log;
mod process;
pub mod structures;
mod trap;
pub mod virtmemory;

extern crate alloc;
use alloc::string::String;
use alloc::vec;
use spin::Once;

use core::arch::global_asm;
use core::panic::PanicInfo;
use core::ptr::write_volatile;

use crate::kernel::{Cpu, Kernel};
use crate::trap::init_trap;
use crate::trap::trampoline::{userret, uservec};
use crate::virtmemory::RAMEND;

const PRIME: &[u8] = include_bytes!("../../user/_prime");
const INIT: &[u8] = include_bytes!("../../user/_init");
const PIPE1: &[u8] = include_bytes!("../../user/_pipe1");
const PIPE2: &[u8] = include_bytes!("../../user/_pipe2");

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

    park:
        j park
    "
);

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

    log::enable(log::INFO);

    init_trap();
    KERNEL.call_once(|| lock::IntMutex::new(Kernel::default()));
    {
        let mut kernel = KERNEL.get().unwrap().lock();

        log::info!("Hello world");

        kernel.init().expect("Kernel init fail");

        kernel.initproc(4).unwrap();
        kernel
            .kvm
            .as_mut()
            .expect("KVM not initialized")
            .start_kvm();
        log::info!("Virt started");

        // Start init
        let user_p0 = kernel.allocproc().unwrap();
        unsafe { user_p0.lock.unlock_manual() };
        user_p0.kexec(String::from("init"), vec![]).unwrap();
        user_p0.state = process::ProcState::Runnable;
    }
    log::info!("into the schedulervere");
    process::scheduler();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    log::logln!("Something went wrong. {:?}", info);
    // shutdown qemu
    unsafe {
        write_volatile(0x100000 as *mut u32, 0x5555);
    }
    loop {}
}

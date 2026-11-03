#![no_std]
#![no_main]
#![feature(allocator_api)]
#![allow(static_mut_refs)]

pub mod allocator;
mod csr;
mod drivers;
pub mod errno;
mod fs;
mod kernel;
pub mod lock;
mod log;
mod process;
pub mod structures;
pub mod vfs;
pub mod virtmemory;

extern crate alloc;
use alloc::{format, string::String, vec};
use spin::Once;

use core::arch::global_asm;
use core::panic::PanicInfo;
use core::ptr::write_volatile;

use crate::kernel::{Cpu, Kernel};
use crate::process::trap::init_trap;
use crate::process::trap::trampoline::{userret, uservec};
use crate::virtmemory::{PAGESIZE, PTE_R, PTE_U, PTE_W, RAMEND, UART};

const UART_IMAGE: &[u8] =
    include_bytes!("../../uart_driver/target/riscv32ima-unknown-none-elf/release/uart_driver");

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

    log::enable(log::INFO | log::ERROR);

    init_trap();
    KERNEL.call_once(|| lock::IntMutex::new(Kernel::default()));
    {
        let mut kernel = KERNEL.get().unwrap().lock();

        log::info!("Hello world");

        kernel.init().expect("Kernel init fail");
        fs::mount_root().expect("ext4 root mount failed");

        kernel.initproc(32).unwrap();
        kernel
            .kvm
            .as_mut()
            .expect("KVM not initialized")
            .start_kvm();
        log::info!("Virt started");

        // Start init
        let init = kernel.allocproc().unwrap();
        unsafe { init.lock.unlock_manual() };
        init.fds = process::initial_tty_fds();
        init.sid = init.pid;
        init.pgid = init.pid;
        process::tty::TTY::set_controlling_session(init.sid.unwrap());
        init.kexec(String::from("/bin/busybox"), vec!["busybox", "sh", "-i"])
            .expect("failed to exec busybox");
        init.state = process::ProcState::Runnable;

        // Start uart server
        let uart = kernel.allocproc().unwrap();
        unsafe { uart.lock.unlock_manual() };
        let uart_address = format!("{UART:#x}");
        uart.kexec_bytes(UART_IMAGE, vec!["uart", &uart_address])
            .expect("failed to exec uart");
        // map the uart page
        uart.pagetable
            .pagetable
            .map(UART, UART, PAGESIZE, PTE_U | PTE_R | PTE_W)
            .expect("failed to map UART");
        uart.state = process::ProcState::Runnable;
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

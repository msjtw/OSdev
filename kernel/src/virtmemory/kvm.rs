
use core::{alloc::Allocator, arch::asm};

use crate::{
    FRAME_ALLOCATOR,
    process::trap::trampoline::_trampoline,
    write_csr,
};

use super::{
    KERNEL_START, PAGE_LAYOUT, PAGESIZE, PTE_R, PTE_W, PTE_X, PageTable, RAMEND, TRAMPOLINE,
    UART, etext,
};

#[derive(Debug)]
pub struct Kvm {
    pagetable: PageTable,
}

impl Kvm {
    pub fn init() -> Result<Kvm, ()> {
        let trampoline = unsafe { &_trampoline as *const usize as usize };
        let pagetable = PageTable::default();
        let mut kvm = Kvm { pagetable };

        // QEMU poweroff register, UART, and virtio-blk.
        kvm.pagetable
            .map(0x00100000, 0x00100000, PAGESIZE, PTE_R | PTE_W)?;
        kvm.pagetable.map(UART, UART, PAGESIZE, PTE_R | PTE_W)?;
        kvm.pagetable
            .map(0x10001000, 0x10001000, 0x8000, PTE_R | PTE_W)?;

        let end_text = unsafe { &etext } as *const usize as usize;
        kvm.pagetable.map(
            KERNEL_START,
            KERNEL_START,
            end_text - KERNEL_START,
            PTE_X | PTE_R,
        )?;
        kvm.pagetable
            .map(end_text, end_text, RAMEND - end_text, PTE_R | PTE_W)?;
        kvm.pagetable
            .map(TRAMPOLINE, trampoline, PAGESIZE, PTE_R | PTE_X)?;
        Ok(kvm)
    }

    pub fn alloc_kstack(&mut self, va: usize) {
        for i in 0..crate::process::KERNEL_STACK_PAGES {
            let kstack_page =
                FRAME_ALLOCATOR.allocate(PAGE_LAYOUT).unwrap().as_ptr() as *mut u8 as usize;
            self.pagetable
                .map(va + i * PAGESIZE, kstack_page, PAGESIZE, PTE_R | PTE_W)
                .unwrap();
        }
    }

    pub fn start_kvm(&self) {
        let satp: usize = self.pagetable.get_satp().into();
        unsafe {
            asm!("sfence.vma zero, zero");
            write_csr!(satp, satp);
            asm!("sfence.vma zero, zero");
        };
    }
}

use core::{
    alloc::GlobalAlloc,
    arch::asm,
    ptr::{NonNull, write_bytes},
};

use alloc::alloc::Allocator;

use crate::{FRAME_ALLOCATOR, HEAP_ALLOCATOR, process::trap::trampoline::_trampoline, write_csr};

use super::{KERNEL_START, PAGE_LAYOUT, PAGESIZE, RAMEND, TRAMPOLINE, UART, etext};

pub const PTE_R: usize = 0b10;
pub const PTE_W: usize = 0b100;
pub const PTE_X: usize = 0b1000;
pub const PTE_U: usize = 0b10000;

#[allow(dead_code)]
#[derive(Debug, Copy, Clone)]
struct Pte {
    pub pa: usize,
    pub ppn: usize,
    pub ppn1: usize,
    pub ppn0: usize,
    pub rsw: u8,
    pub d: bool,
    pub a: bool,
    pub g: bool,
    pub u: bool,
    pub x: bool,
    pub w: bool,
    pub r: bool,
    pub v: bool,
    pub perm: usize,
}

impl From<usize> for Pte {
    fn from(pte: usize) -> Self {
        Pte {
            pa: (pte & 0b11111111111111111111110000000000) << 2,
            ppn: (pte & 0b11111111111111111111110000000000) >> 10,
            ppn1: (pte & 0b11111111111100000000000000000000) >> 20,
            ppn0: (pte & 0b00000000000011111111110000000000) >> 10,
            rsw: ((pte & 0b00000000000000000000001100000000) >> 8) as u8,
            d: (pte & 0b00000000000000000000000010000000) >= 1,
            a: (pte & 0b00000000000000000000000001000000) >= 1,
            g: (pte & 0b00000000000000000000000000100000) >= 1,
            u: (pte & 0b00000000000000000000000000010000) >= 1,
            x: (pte & 0b00000000000000000000000000001000) >= 1,
            w: (pte & 0b00000000000000000000000000000100) >= 1,
            r: (pte & 0b00000000000000000000000000000010) >= 1,
            v: (pte & 0b00000000000000000000000000000001) >= 1,
            perm: (pte & 0b11110),
        }
    }
}

impl From<Pte> for usize {
    fn from(val: Pte) -> Self {
        val.ppn << 10
            | (val.rsw as usize) << 8
            | (val.d as usize) << 7
            | (val.a as usize) << 6
            | (val.g as usize) << 5
            | (val.u as usize) << 4
            | (val.x as usize) << 3
            | (val.w as usize) << 2
            | (val.r as usize) << 1
            | (val.v as usize)
    }
}

impl Pte {
    #[inline]
    fn from_addr(pa: usize) -> Pte {
        let mask = (1 << 12) - 1;
        let pte = (pa & !mask) >> 2;
        Pte::from(pte)
    }
}

#[derive(Debug)]
pub struct SATP {
    mode: usize,
    asid: usize,
    ppn: usize,
}

impl From<SATP> for usize {
    fn from(val: SATP) -> Self {
        let mut satp: usize = 0;
        satp |= val.mode << 31;
        satp |= val.asid << 22;
        satp |= val.ppn;
        satp
    }
}

#[derive(Debug)]
struct VA {
    vpn1: usize,
    vpn0: usize,
}

impl VA {
    fn vpn(&self, level: usize) -> Option<usize> {
        match level {
            0 => Some(self.vpn0),
            1 => Some(self.vpn1),
            _ => None,
        }
    }
}

impl From<usize> for VA {
    fn from(val: usize) -> Self {
        VA {
            vpn1: (val & 0b11111111110000000000000000000000) >> 22,
            vpn0: (val & 0b00000000001111111111000000000000) >> 12,
        }
    }
}

#[derive(Debug)]
pub struct PageTable {
    root: NonNull<usize>,
}

unsafe impl Send for PageTable {}

impl Default for PageTable {
    fn default() -> PageTable {
        PageTable::new()
    }
}

impl Drop for PageTable {
    fn drop(&mut self) {
        self.free();
    }
}

impl PageTable {
    fn new() -> PageTable {
        let ptr = unsafe { HEAP_ALLOCATOR.alloc_zeroed(PAGE_LAYOUT) as *mut usize };
        unsafe { write_bytes(ptr, 0, 1024) };
        let pagetable = NonNull::new(ptr).expect("failed to allocate root page table");
        Self { root: pagetable }
    }

    fn free(&mut self) {
        PageTable::free_req(self.root);
        unsafe { HEAP_ALLOCATOR.dealloc(self.root.as_ptr() as *mut u8, PAGE_LAYOUT) };
    }

    fn free_req(root: NonNull<usize>) {
        for i in 0..1024 {
            let pte = unsafe { root.add(i).read() };
            let pte = Pte::from(pte);
            if pte.v {
                if !pte.r && !pte.w && !pte.x {
                    let root = NonNull::new((pte.ppn << 12) as *mut usize).unwrap();
                    PageTable::free_req(root);
                    // walk() allocates page-table nodes from the heap.
                    unsafe { HEAP_ALLOCATOR.dealloc(root.as_ptr() as *mut u8, PAGE_LAYOUT) };
                } else {
                    panic!(
                        "unmapping allocated page 0x{:x} perm: 0b{:b}",
                        pte.pa, pte.perm
                    );
                }
            }
        }
    }

    // Returns the leaf PTE address for a virtual address.
    fn walk(&self, virt_a: usize, walk_type: WalkType) -> Option<NonNull<usize>> {
        let va = VA::from(virt_a);
        let index = va.vpn(1)?;
        let pte_addr = unsafe { self.root.as_ptr().add(index) };
        let pte = Pte::from(unsafe { pte_addr.read() });

        let a: NonNull<usize>;
        if pte.v {
            a = NonNull::new((pte.ppn << 12) as *mut usize)?;
        } else {
            if walk_type == WalkType::Walk {
                return None;
            }
            let new_page = unsafe { HEAP_ALLOCATOR.alloc_zeroed(PAGE_LAYOUT) as *mut usize };
            let new_page = NonNull::new(new_page)?;
            unsafe { write_bytes(new_page.as_ptr(), 0, 1024) };
            let mut new_pte = Pte::from_addr(new_page.as_ptr() as usize);
            new_pte.v = true;
            let tmp: usize = new_pte.into();
            unsafe { pte_addr.write(tmp) };
            a = new_page;
        }

        let index = va.vpn(0)?;
        let pte_addr = unsafe { a.add(index) };
        Some(pte_addr)
    }

    // Return the backing page base and permission bits of a valid entry.
    pub(super) fn lookup(&self, addr: usize) -> Option<(usize, usize)> {
        let entry = self.walk(addr, WalkType::Walk)?;
        let pte = Pte::from(unsafe { entry.read() });
        pte.v.then_some((pte.pa, pte.perm))
    }

    pub(super) fn add_permissions(&mut self, addr: usize, perm: usize) -> Result<(), ()> {
        let entry = self.walk(addr, WalkType::Walk).ok_or(())?;
        unsafe { entry.write(entry.read() | perm) };
        Ok(())
    }

    pub(super) fn get_satp(&self) -> SATP {
        SATP {
            mode: 1,
            asid: 0,
            ppn: (self.root.as_ptr() as usize) >> 12,
        }
    }

    pub(super) fn map(&mut self, virt: usize, phys: usize, size: usize, perm: usize) -> Result<(), ()> {
        if !phys.is_multiple_of(PAGESIZE) {
            panic!("mapping to unalinged frame 0x{:08x}\n", phys);
        }
        if !virt.is_multiple_of(PAGESIZE) {
            panic!("mapping unalinged page 0x{:08x}\n", virt);
        }
        if !size.is_multiple_of(PAGESIZE) {
            panic!("mapping not whole pages\n");
        }

        let mut vaddr = virt;
        let mut paddr = phys;
        let vaddr_end = virt + size;
        while vaddr < vaddr_end {
            let pte_addr = self.walk(vaddr, WalkType::Alloc).ok_or(())?;
            let mut pte = Pte::from_addr(paddr);
            pte.v = true;
            let mut pte: usize = pte.into();
            pte |= perm;
            unsafe { pte_addr.write(pte) };
            vaddr += PAGESIZE;
            paddr += PAGESIZE;
        }
        Ok(())
    }

    // Release user frames if requested, but retain internal page-table nodes.
    pub(super) fn unmap(&mut self, virt: usize, size: usize, free: bool) -> Result<(), ()> {
        if !size.is_multiple_of(PAGESIZE) {
            return Err(());
        }

        let mut va = virt;
        for _ in 0..(size / PAGESIZE) {
            if let Some(pte_addr) = self.walk(va, WalkType::Walk) {
                let pte = Pte::from(unsafe { pte_addr.read() });
                if pte.v {
                    let is_leaf = pte.r || pte.w || pte.x;
                    // Kernel/static/device mappings are not owned user frames.
                    if free && is_leaf && pte.u {
                        let page = (pte.ppn << 12) as *mut u8;
                        unsafe { HEAP_ALLOCATOR.dealloc(page, PAGE_LAYOUT) };
                    }
                    unsafe { pte_addr.write(0) };
                    unsafe { asm!("sfence.vma {}, zero", in(reg) va) };
                }
            }
            va += PAGESIZE;
        }
        Ok(())
    }
}

#[derive(PartialEq, Eq)]
enum WalkType {
    Alloc,
    Walk,
}

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

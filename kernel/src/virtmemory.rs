use core::{
    alloc::{GlobalAlloc, Layout},
    arch::asm,
    ptr::{NonNull, copy_nonoverlapping, write_bytes},
};

use alloc::{alloc::Allocator, string::String, vec::Vec};

use crate::{
    FRAME_ALLOCATOR, HEAP_ALLOCATOR, 
    process::{
        Process,
        trap::{trampoline::_trampoline, trapframe::Trapframe},
    },
    write_csr,
};

unsafe extern "C" {
    pub static etext: usize;
    pub static ekernel: usize;
    pub static _STACK_PTR: usize;
}

pub const PAGESIZE: usize = 4 * 1024;

pub fn align_up(value: usize, alignment: usize) -> Option<usize> {
    let adjustment = alignment.checked_sub(1)?;
    value
        .checked_add(adjustment)
        .map(|value| value / alignment * alignment)
}

const RAMSIZE: usize = 62 * 1024 * 1024;
const RAMSTART: usize = 0x80200000;
pub const RAMEND: usize = RAMSTART + RAMSIZE;

const KERNEL_START: usize = 0x80200000;
pub const USER_START: usize = 0x10000;
pub const UART: usize = 0x10000000;

// NOTE: I need address one above last virutal, but it wont fit in u32 (it is 33 bit).
// So the last page is discarded and VIRT_END is set to first address of last page.
// 0xffffff is last address of last page, -PAGESIZE is last of one to last page, +1 is first of last
pub const VIRT_END: usize = 0xffffffff - PAGESIZE + 1;
// pub const VIRT_END: u32 = u32::MAX;
pub const TRAMPOLINE: usize = VIRT_END - PAGESIZE;
pub const TRAPFRAME: usize = TRAMPOLINE - PAGESIZE;

pub const USER_MMAP_BASE: usize = 0x4000_0000;
pub const USER_STACK_TOP: usize = TRAPFRAME;
pub const USER_STACK_PAGES: usize = 8;
// Keep one unmapped guard page below the fixed user stack.
pub const USER_MMAP_END: usize = USER_STACK_TOP - (USER_STACK_PAGES + 1) * PAGESIZE;

pub const PAGE_LAYOUT: Layout = unsafe { Layout::from_size_align_unchecked(PAGESIZE, PAGESIZE) };

pub const PTE_R: usize = 0b10;
pub const PTE_W: usize = 0b100;
pub const PTE_X: usize = 0b1000;
pub const PTE_U: usize = 0b10000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmError {
    Invalid,
    NoMemory,
}

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
    // get pte from physical address without permissions
    fn from_addr(pa: usize) -> Pte {
        let mask = (1 << 12) - 1;
        let pte = (pa & !mask) >> 2;
        Pte::from(pte)
    }

    // fn set_perm(&mut self, perm: &Perm) {
    //     self.r = perm.r;
    //     self.w = perm.w;
    //     self.x = perm.x;
    // }
}

struct Perm {
    r: bool,
    w: bool,
    x: bool,
}

impl From<Perm> for usize {
    fn from(val: Perm) -> Self {
        let mut res = 0;
        if val.r {
            res |= 0b10;
        }
        if val.w {
            res |= 0b100;
        }
        if val.x {
            res |= 0b1000;
        }
        res
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
    // offset: usize,
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
            // offset: val & 0b00000000000000000000111111111111,
        }
    }
}

#[derive(Debug)]
struct PA {
    ppn1: usize,
    ppn0: usize,
    offset: usize,
}

impl From<PA> for usize {
    fn from(val: PA) -> Self {
        let ppn1 = val.ppn1 << 22;
        let ppn0 = val.ppn0 << 12;
        ppn1 | ppn0 | val.offset
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
        // free all 2nd level page tables
        // there are 1024 PTES in pagetable
        PageTable::free_req(self.root);

        // free root
        unsafe { HEAP_ALLOCATOR.dealloc(self.root.as_ptr() as *mut u8, PAGE_LAYOUT) };
    }

    fn free_req(root: NonNull<usize>) {
        for i in 0..1024 {
            let pte = unsafe { root.add(i).read() };
            let pte = Pte::from(pte);
            if pte.v {
                if !pte.r && !pte.w && !pte.x {
                    // lower level page table
                    let root = NonNull::new((pte.ppn << 12) as *mut usize).unwrap();
                    PageTable::free_req(root);
                    // walk() allocates page-table nodes from the heap.
                    unsafe { HEAP_ALLOCATOR.dealloc(root.as_ptr() as *mut u8, PAGE_LAYOUT) };
                } else {
                    // there is some unmapped allocaed page
                    panic!(
                        "unmapping allocated page 0x{:x} perm: 0b{:b}",
                        pte.pa, pte.perm
                    );
                }
            }
        }
    }

    // returns leaf pte addr for given virtual address
    // with support for megapages
    fn walk(&self, virt_a: usize, walk_type: WalkType) -> Option<NonNull<usize>> {
        let va = VA::from(virt_a);

        let index = va.vpn(1)?;
        let pte_addr = unsafe { self.root.as_ptr().add(index) };
        let pte = unsafe { pte_addr.read() };

        let pte = Pte::from(pte);

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

    // map virtual memory range to physical memory range
    fn map(&mut self, virt: usize, phys: usize, size: usize, perm: usize) -> Result<(), ()> {
        // TODO: tests
        // - size and virt addr aligned on page
        // - size > 0 and end < RAMEND

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
            let pte_addr = self.walk(vaddr as usize, WalkType::Alloc).ok_or(())?;
            // NOTE: check for remap (I don't think it's possible)

            let mut pte = Pte::from_addr(paddr);
            pte.v = true;
            let mut pte: usize = pte.into(); // set permissions
            pte |= perm;
            unsafe { pte_addr.write(pte) };

            vaddr += PAGESIZE;
            paddr += PAGESIZE;
        }
        Ok(())
    }

    // remove mappings from virt to virt+size
    // if free it will also free the mapped pages but not the internal tree pages
    fn unmap(&mut self, virt: usize, size: usize, free: bool) -> Result<(), ()> {
        if !size.is_multiple_of(PAGESIZE) {
            return Err(());
        }

        let mut va = virt;
        for _ in 0..(size / PAGESIZE) {
            if let Some(pte_addr) = self.walk(va as usize, WalkType::Walk) {
                let pte = Pte::from(unsafe { pte_addr.read() });
                if pte.v {
                    let is_leaf = pte.r || pte.w || pte.x;

                    // FIX: Free only user leaf mappings. Kernel/static/device mappings may point
                    // to non-heap storage and must be unmapped without dealloc.
                    if free && is_leaf && pte.u {
                        let page = (pte.ppn << 12) as *mut u8;
                        unsafe { HEAP_ALLOCATOR.dealloc(page, PAGE_LAYOUT) };
                    }

                    unsafe { pte_addr.write(0) };
                    // Remove a stale translation before this virtual page can
                    // be returned to the user allocator.
                    unsafe { asm!("sfence.vma {}, zero", in(reg) va) };
                }
            }

            va += PAGESIZE;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct Kvm {
    pagetable: PageTable,
}

impl Kvm {
    // NOTE: Top of kernel address space is:
    // trampoline
    // guard
    // kernel0
    // guard
    // ...

    pub fn init() -> Result<Kvm, ()> {
        let trampoline = unsafe { &_trampoline as *const usize as usize };

        let pagetable = PageTable::default();
        let mut kvm = Kvm { pagetable };
        // map all sections

        // QEMU poweroff register.
        kvm.pagetable
            .map(0x00100000, 0x00100000, PAGESIZE, PTE_R | PTE_W)?;

        // uart
        kvm.pagetable.map(UART, UART, PAGESIZE, PTE_R | PTE_W)?;

        // virtio-blk
        kvm.pagetable
            .map(0x10001000, 0x10001000, 0x8000, PTE_R | PTE_W)?;

        // kernel text
        let end_text = unsafe { &etext } as *const usize as usize;
        kvm.pagetable.map(
            KERNEL_START,
            KERNEL_START,
            end_text - KERNEL_START,
            PTE_X | PTE_R,
        )?;

        // kernel data and ram after kernel
        kvm.pagetable
            .map(end_text, end_text, RAMEND - end_text, PTE_R | PTE_W)?;

        // map trampoline
        kvm.pagetable
            .map(TRAMPOLINE, trampoline, PAGESIZE, PTE_R | PTE_X)?;

        Ok(kvm)
    }

    // maps and allocates kernel stacks
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
        let ppn = (self.pagetable.root.as_ptr() as usize) >> 12;
        let satp = SATP {
            mode: 1,
            asid: 0,
            ppn,
        };
        let satp: usize = satp.into();
        unsafe {
            asm!("sfence.vma zero, zero");
            write_csr!(satp, satp);
            asm!("sfence.vma zero, zero");
        };
    }

    // Cretae Ptes for translaition virt -> phys
    // continous virt to virt + size to continous phys to phys + size
}

#[derive(Debug)]
pub struct Uvm {
    pub end: usize,
    pub heap_base: usize,
    pub prog_break: usize,
    pub mmap_next: usize,
    page_list: Vec<usize>,
    size: usize,
    pagetable: PageTable,
}

impl Clone for Uvm {
    fn clone(&self) -> Self {
        let mut vm = Uvm::new().unwrap();

        vm.page_list = self.page_list.clone();
        vm.size = self.size;
        vm.end = self.end;
        vm.heap_base = self.heap_base;
        vm.prog_break = self.prog_break;
        vm.mmap_next = self.mmap_next;

        // for addr in (USER_START..self.end()).step_by(PAGESIZE) {
        for addr in &self.page_list {
            let pte = unsafe { self.pagetable.walk(*addr, WalkType::Walk).unwrap().read() };
            let pte = Pte::from(pte);
            if !pte.v {
                continue;
            }
            let from = pte.pa as *const u8;
            let to = FRAME_ALLOCATOR.allocate(PAGE_LAYOUT).unwrap().as_ptr() as *mut u8;
            unsafe { copy_nonoverlapping(from, to, PAGESIZE) };

            vm.pagetable
                .map(*addr, to as usize, PAGESIZE, pte.perm)
                .unwrap();
        }

        vm
    }
}

impl Drop for Uvm {
    fn drop(&mut self) {
        self.free();
    }
}

// The address space is continuous and starts at virt 0x80000000
impl Uvm {
    pub fn new() -> Result<Uvm, ()> {
        let uvm = Uvm {
            end: 0,
            heap_base: 0,
            prog_break: 0,
            mmap_next: USER_MMAP_BASE,
            page_list: Vec::new(),
            size: 0,
            pagetable: PageTable::default(),
        };
        Ok(uvm)
    }

    pub fn free(&mut self) {
        self.pagetable.unmap(TRAMPOLINE, PAGESIZE, false).unwrap();
        self.pagetable.unmap(TRAPFRAME, PAGESIZE, false).unwrap();

        // this frees all pages in this vm but leaves page tree structure
        for addr in &self.page_list {
            self.pagetable.unmap(*addr, PAGESIZE, true).unwrap();
        }
    }

    pub fn get_satp(&self) -> SATP {
        let ppn = (self.pagetable.root.as_ptr() as usize) >> 12;
        SATP {
            mode: 1,
            asid: 0,
            ppn,
        }
    }

    pub fn brk(&mut self, addr: usize) -> usize {
        if addr == 0 || addr < self.heap_base {
            return self.prog_break;
        }

        let Some(old_end) = align_up(self.prog_break, PAGESIZE) else {
            return self.prog_break;
        };
        let Some(new_end) = align_up(addr, PAGESIZE) else {
            return self.prog_break;
        };
        if new_end >= USER_MMAP_BASE {
            return self.prog_break;
        }

        let result = if new_end > old_end {
            self.map(old_end, new_end - old_end, PTE_R | PTE_W)
        } else if new_end < old_end {
            self.unmap(new_end, old_end - new_end)
        } else {
            Ok(())
        };

        if result.is_ok() {
            self.prog_break = addr;
        }
        self.prog_break
    }

    // Map freshly allocated, zero-filled user pages at a fixed address.
    pub fn map(&mut self, addr: usize, size: usize, perm: usize) -> Result<(), VmError> {
        if size == 0
            || perm == 0
            || !addr.is_multiple_of(PAGESIZE)
            || !size.is_multiple_of(PAGESIZE)
            || addr.checked_add(size).is_none()
        {
            return Err(VmError::Invalid);
        }
        self.alloc(addr, size, perm).map_err(|()| VmError::NoMemory)
    }

    // ELF LOAD segments can share pages. Reuse them instead of overwriting
    // mappings and losing their backing frames; combine segment permissions.
    pub fn map_elf_segment(&mut self, addr: usize, size: usize, perm: usize) -> Result<(), VmError> {
        if perm == 0 {
            return Err(VmError::Invalid);
        }
        let end = addr.checked_add(size).ok_or(VmError::Invalid)?;
        for page in (addr..end).step_by(PAGESIZE) {
            if let Some(entry) = self.pagetable.walk(page, WalkType::Walk) {
                let bits = unsafe { entry.as_ptr().read() };
                if Pte::from(bits).v {
                    if !Pte::from(bits).u {
                        return Err(VmError::Invalid);
                    }
                    unsafe { entry.as_ptr().write(bits | perm) };
                    continue;
                }
            }
            self.map(page, PAGESIZE, perm)?;
        }
        Ok(())
    }

    // Allocate an mapping in the mmap region and return its base.
    pub fn map_anonymous(
        &mut self,
        addr: usize,
        length: usize,
        perm: usize,
    ) -> Result<usize, VmError> {
        if addr != 0 || length == 0 || perm == 0 {
            return Err(VmError::Invalid);
        }
        let length = align_up(length, PAGESIZE).ok_or(VmError::NoMemory)?;
        let base = self.mmap_next;
        let next = base.checked_add(length).ok_or(VmError::NoMemory)?;
        if next > USER_MMAP_END {
            return Err(VmError::NoMemory);
        }

        self.map(base, length, perm)?;
        self.mmap_next = next;
        Ok(base)
    }

    // Remove user pages and release their backing frames.
    pub fn unmap(&mut self, addr: usize, length: usize) -> Result<(), VmError> {
        if length == 0 || !addr.is_multiple_of(PAGESIZE) {
            return Err(VmError::Invalid);
        }
        let size = align_up(length, PAGESIZE).ok_or(VmError::Invalid)?;
        addr.checked_add(size).ok_or(VmError::Invalid)?;
        self.dealloc(addr, size).map_err(|()| VmError::Invalid)
    }

    pub fn init_proc(&mut self, proc: &Process) -> Result<(), ()> {
        let trampoline = unsafe { &_trampoline as *const usize as usize };
        self.pagetable
            .map(TRAMPOLINE, trampoline, PAGESIZE, PTE_R | PTE_X)?;

        self.pagetable.map(
            TRAPFRAME,
            proc.trapframe.as_ref() as *const Trapframe as usize,
            PAGESIZE,
            PTE_R | PTE_W,
        )?;

        Ok(())
    }

    // copy img to self at va
    // memory needs to be preallocated  TODO: does it really? Why can't it be allocated here?
    pub fn load(&mut self, mut va: usize, mut img: &[u8]) -> Result<(), ()> {
        // load is executed in kernel with kernel pagetree.
        while !img.is_empty() {
            let page_va = va & !(PAGESIZE - 1);
            let page_offset = va - page_va;
            let count = (PAGESIZE - page_offset).min(img.len());
            let pte = unsafe {
                self.pagetable
                    .walk(page_va, WalkType::Walk)
                    .ok_or(())?
                    .read()
            };
            let pte = Pte::from(pte);
            // The pages were zeroed by alloc(), so copying only file bytes
            // naturally leaves an ELF segment's BSS tail zero-filled.
            unsafe {
                let src_addr = img.as_ptr();
                let dst_addr = (pte.pa + page_offset) as *mut u8;
                copy_nonoverlapping(src_addr, dst_addr, count);
            };
            va += count;
            img = &img[count..];
        }
        Ok(())
    }

    // grow new pages to size
    // it creates virt address space from USERBASE to size
    // allocates memory at virtual address addr of size
    fn alloc(&mut self, addr: usize, size: usize, perm: usize) -> Result<(), ()> {
        let mut allocated = 0;
        let original_end = self.end;
        while allocated < size {
            // NOTE: Gurad pages shoudl not be mapped
            if perm == 0 {
                self.size += PAGESIZE;
                allocated += PAGESIZE;
                continue;
            }

            let page = unsafe { HEAP_ALLOCATOR.alloc_zeroed(PAGE_LAYOUT) as usize };
            // NOTE: OOM Error
            if page == 0 {
                self.rollback_alloc(addr, allocated, original_end);
                return Err(());
            }

            let Some(alloc_addr) = addr.checked_add(allocated) else {
                unsafe { HEAP_ALLOCATOR.dealloc(page as *mut u8, PAGE_LAYOUT) };
                self.rollback_alloc(addr, allocated, original_end);
                return Err(());
            };
            if self
                .pagetable
                .map(alloc_addr, page, PAGESIZE, perm | PTE_U)
                .is_err()
            {
                unsafe { HEAP_ALLOCATOR.dealloc(page as *mut u8, PAGE_LAYOUT) };
                self.rollback_alloc(addr, allocated, original_end);
                return Err(());
            }
            // Keep virtual page addresses: clone() walks these VAs, and free()
            // unmaps them from this page table.
            self.page_list.push(alloc_addr);
            self.size += PAGESIZE;
            allocated += PAGESIZE;
            self.end = self.end.max(alloc_addr + PAGESIZE);
        }
        Ok(())
    }

    fn rollback_alloc(&mut self, addr: usize, allocated: usize, original_end: usize) {
        if allocated != 0 {
            self.dealloc(addr, allocated)
                .expect("allocated user pages must be mappable for rollback");
        }
        self.end = original_end;
    }

    // Unmap a page-aligned range and release its backing user frames.
    fn dealloc(&mut self, addr: usize, size: usize) -> Result<(), ()> {
        if size == 0 || !addr.is_multiple_of(PAGESIZE) || !size.is_multiple_of(PAGESIZE) {
            return Err(());
        }
        let end = addr.checked_add(size).ok_or(())?;

        self.pagetable.unmap(addr, size, true)?;

        let pages_before = self.page_list.len();
        self.page_list.retain(|&page| page < addr || page >= end);
        let pages_removed = pages_before - self.page_list.len();
        self.size = self.size.saturating_sub(pages_removed * PAGESIZE);

        Ok(())
    }
}

#[derive(PartialEq, Eq)]
enum WalkType {
    Alloc,
    Walk,
}

#[derive(Clone, Copy)]
enum UserAccess {
    Read,
    Write,
}

// Translate user memory only. ELF loading uses Uvm::load separately so it can
// populate executable/read-only pages without granting users write access.
fn walkaddr(pagetree: &mut PageTable, virt_a: usize, access: UserAccess) -> Option<usize> {
    let pte = unsafe { pagetree.walk(virt_a, WalkType::Walk).ok_or(()).ok()?.read() };
    let pte = Pte::from(pte);
    if !pte.v || !pte.u || !(pte.r || pte.x) || (pte.w && !pte.r) {
        return None;
    }
    match access {
        UserAccess::Read if !pte.r => return None,
        UserAccess::Write if !pte.w => return None,
        _ => {}
    }
    let pa = pte.pa + (virt_a % PAGESIZE as usize);
    Some(pa)
}

// copy from given address space INTO current
pub fn copy_in<T: Clone>(uv: &mut Uvm, addr: usize) -> Result<T, ()> {
    let size = size_of::<T>();
    let bytes = copy_in_bytes(uv, addr, size)?;
    let value = unsafe { bytes.as_ptr().cast::<T>().read_unaligned() };
    Ok(value)
}

pub fn copy_in_cont<T: Copy>(uv: &mut Uvm, addr: usize, count: usize) -> Result<Vec<T>, ()> {
    let size = size_of::<T>();
    let bytes = copy_in_bytes(uv, addr, count.checked_mul(size).ok_or(())?)?;
    let mut values = Vec::new();
    values.try_reserve_exact(count).map_err(|_| ())?;
    for i in 0..count {
        let value = unsafe { bytes.as_ptr().add(i * size).cast::<T>().read_unaligned() };
        values.push(value);
    }
    Ok(values)
}

pub fn copy_in_str(uv: &mut Uvm, mut addr: usize) -> Result<String, ()> {
    let mut bytes = Vec::new();
    loop {
        let mut phys = walkaddr(&mut uv.pagetable, addr, UserAccess::Read).ok_or(())?;
        loop {
            let byte = unsafe { (phys as *const u8).read() };
            if byte == 0 {
                return String::from_utf8(bytes).map_err(|_| ());
            }
            bytes.push(byte);
            addr = addr.checked_add(1).ok_or(())?;
            phys += 1;
            if addr % PAGESIZE == 0 {
                break;
            }
        }
    }
}

// copy from current OUT to user
pub fn copy_out<T>(uv: &mut Uvm, addr: usize, data: T) -> Result<(), ()> {
    unsafe { copy_out_bytes(uv, addr, (&data as *const T).cast(), size_of::<T>()) }
}

pub fn copy_out_cont<T: Copy>(uv: &mut Uvm, addr: usize, data: &[T]) -> Result<(), ()> {
    let len = data.len().checked_mul(size_of::<T>()).ok_or(())?;
    unsafe { copy_out_bytes(uv, addr, data.as_ptr().cast(), len) }
}

fn copy_in_bytes(uv: &mut Uvm, mut addr: usize, len: usize) -> Result<Vec<u8>, ()> {
    let mut remaining = len;
    if len == 0 {
        return Ok(Vec::new());
    }
    let end = addr.checked_add(remaining).ok_or(())?;
    if end > TRAPFRAME {
        return Err(());
    }

    let mut bytes = Vec::<u8>::new();
    // small optimisation, this reserves the space but doest set len
    bytes.try_reserve_exact(len).map_err(|_| ())?;
    let mut bytes_ptr = bytes.as_mut_ptr();
    while remaining != 0 {
        let phys = walkaddr(&mut uv.pagetable, addr, UserAccess::Read).ok_or(())?;
        let chunk = remaining.min(PAGESIZE - addr % PAGESIZE);
        unsafe {
            // copy straight into bytes
            copy_nonoverlapping(phys as *const u8, bytes_ptr, chunk);
            bytes_ptr = bytes_ptr.add(chunk);
        }
        addr = addr.checked_add(chunk).ok_or(())?;
        remaining -= chunk;
    }
    // everything successful so set it's length
    unsafe { bytes.set_len(len) };
    Ok(bytes)
}

unsafe fn copy_out_bytes(
    uv: &mut Uvm,
    mut addr: usize,
    mut source: *const core::mem::MaybeUninit<u8>, // data that may be uninitialized
    len: usize,
) -> Result<(), ()> {
    if len == 0 {
        return Ok(());
    }
    let end = addr.checked_add(len).ok_or(())?;
    if end > TRAPFRAME {
        return Err(());
    }

    let mut remaining = len;
    while remaining != 0 {
        let phys = walkaddr(&mut uv.pagetable, addr, UserAccess::Write).ok_or(())?;
        let chunk = remaining.min(PAGESIZE - addr % PAGESIZE);
        unsafe {
            copy_nonoverlapping(source, phys as *mut core::mem::MaybeUninit<u8>, chunk);
            source = source.add(chunk);
        }
        addr = addr.checked_add(chunk).ok_or(())?;
        remaining -= chunk;
    }
    Ok(())
}

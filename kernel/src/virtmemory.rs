//! Address-space policy and user-memory copying, backed by Sv32 page tables.

use core::{
    alloc::{GlobalAlloc, Layout},
    ptr::copy_nonoverlapping,
};

use alloc::{alloc::Allocator, string::String, vec::Vec};

use crate::{
    FRAME_ALLOCATOR, HEAP_ALLOCATOR,
    process::{
        Process,
        trap::{trampoline::_trampoline, trapframe::Trapframe},
    },
};

mod sv32;

pub use sv32::{Kvm, PageTable, SATP, PTE_R, PTE_U, PTE_W, PTE_X};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmError {
    Invalid,
    NoMemory,
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
            let Some((pa, perm)) = self.pagetable.lookup(*addr) else {
                continue;
            };
            let from = pa as *const u8;
            let to = FRAME_ALLOCATOR.allocate(PAGE_LAYOUT).unwrap().as_ptr() as *mut u8;
            unsafe { copy_nonoverlapping(from, to, PAGESIZE) };

            vm.pagetable
                .map(*addr, to as usize, PAGESIZE, perm)
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
        self.pagetable.get_satp()
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
            if let Some((_, permissions)) = self.pagetable.lookup(page) {
                if permissions & PTE_U == 0 {
                    return Err(VmError::Invalid);
                }
                self.pagetable
                    .add_permissions(page, perm)
                    .map_err(|_| VmError::Invalid)?;
                continue;
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
            let (pa, _) = self.pagetable.lookup(page_va).ok_or(())?;
            // The pages were zeroed by alloc(), so copying only file bytes
            // naturally leaves an ELF segment's BSS tail zero-filled.
            unsafe {
                let src_addr = img.as_ptr();
                let dst_addr = (pa + page_offset) as *mut u8;
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

#[derive(Clone, Copy)]
enum UserAccess {
    Read,
    Write,
}

// Translate user memory only. ELF loading uses Uvm::load separately so it can
// populate executable/read-only pages without granting users write access.
fn walkaddr(pagetree: &mut PageTable, virt_a: usize, access: UserAccess) -> Option<usize> {
    let (page, perm) = pagetree.lookup(virt_a)?;
    if perm & PTE_U == 0 || perm & (PTE_R | PTE_X) == 0 || (perm & PTE_W != 0 && perm & PTE_R == 0) {
        return None;
    }
    match access {
        UserAccess::Read if perm & PTE_R == 0 => return None,
        UserAccess::Write if perm & PTE_W == 0 => return None,
        _ => {}
    }
    let pa = page + (virt_a % PAGESIZE);
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

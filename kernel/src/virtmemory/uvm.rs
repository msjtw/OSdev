use core::{
    alloc::GlobalAlloc,
    ptr::copy_nonoverlapping,
};

use alloc::{alloc::Allocator, vec::Vec};

use crate::{
    FRAME_ALLOCATOR, HEAP_ALLOCATOR,
    process::{
        Process,
        trap::{trampoline::_trampoline, trapframe::Trapframe},
    },
};

use super::{
    PAGE_LAYOUT, PAGESIZE, PTE_R, PTE_U, PTE_W, PTE_X, PageTable, SATP, TRAMPOLINE, TRAPFRAME,
    USER_MMAP_BASE, USER_MMAP_END, VmError, align_up,
};

#[derive(Debug)]
pub struct Uvm {
    pub end: usize,
    pub heap_base: usize,
    pub prog_break: usize,
    pub mmap_next: usize,
    page_list: Vec<usize>,
    size: usize,
    pub(super) pagetable: PageTable,
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
        self.free()
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
    pub fn map_elf_segment(
        &mut self,
        addr: usize,
        size: usize,
        perm: usize,
    ) -> Result<(), VmError> {
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

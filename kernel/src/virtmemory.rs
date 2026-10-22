mod kvm;
mod sv32;
mod uvm;

use core::{alloc::Layout, ptr::copy_nonoverlapping};

use alloc::{string::String, vec::Vec};

pub use kvm::Kvm;
pub use sv32::{PTE_R, PTE_U, PTE_W, PTE_X, PageTable, SATP};
pub use uvm::Uvm;

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
    let (page, perm) = pagetree.lookup(virt_a)?;
    if perm & PTE_U == 0 || perm & (PTE_R | PTE_X) == 0 || (perm & PTE_W != 0 && perm & PTE_R == 0)
    {
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

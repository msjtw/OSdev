use core::{
    alloc::{GlobalAlloc, Layout},
    arch::asm,
    ptr::{NonNull, copy_nonoverlapping},
};

use alloc::{alloc::Allocator, string::String, vec::Vec};

use crate::{
    FRAME_ALLOCATOR, HEAP_ALLOCATOR,
    process::{Process, trapframe::Trapframe},
    trap::trampoline::_trampoline,
    write_csr,
};

unsafe extern "C" {
    pub static etext: usize;
    pub static ekernel: usize;
    pub static _STACK_PTR: usize;
}

pub const PAGESIZE: usize = 4 * 1024;
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

pub const PAGE_LAYOUT: Layout = unsafe { Layout::from_size_align_unchecked(PAGESIZE, PAGESIZE) };

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
            perm: (pte & 0b1110),
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

pub struct PageTable {
    root: NonNull<usize>,
}

unsafe impl Send for PageTable {}

impl Default for PageTable {
    fn default() -> PageTable {
        let ptr = unsafe { HEAP_ALLOCATOR.alloc(PAGE_LAYOUT) as *mut usize };

        let pagetable = NonNull::new(ptr).expect("failed to allocate root page table");

        Self { root: pagetable }
    }
}

impl PageTable {
    // returns leaf pte addr for given virtual address
    // with support for megapages
    fn walk(&self, virt_a: usize, walk_type: WalkType) -> Option<NonNull<usize>> {
        let va = VA::from(virt_a);

        let index = va.vpn(1)? ;
        let pte_addr = unsafe { self.root.as_ptr().add(index) };
        let pte_u32 = unsafe { pte_addr.read() };

        let pte = Pte::from(pte_u32);

        let a: PageTable;
        if pte.v {
            let root = NonNull::new((pte.ppn << 12) as *mut usize)?;
            a = PageTable { root };
        } else {
            if walk_type == WalkType::Walk {
                return None;
            }
            let new_page = unsafe { HEAP_ALLOCATOR.alloc(PAGE_LAYOUT) as *mut usize };
            let mut new_pte = Pte::from_addr(new_page as usize);
            new_pte.v = true;
            unsafe { pte_addr.write(new_pte.into()) };
            let root = NonNull::new(new_page)?;
            a = PageTable { root };
        }

        let index = va.vpn(0)?;
        let pte_addr = unsafe { a.root.add(index) };

        Some(pte_addr)
    }
}

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

        // uart
        map(&mut kvm.pagetable, UART, UART, PAGESIZE, PTE_R | PTE_W)?;

        // kernel text
        let end_text = unsafe { &etext } as *const usize as usize;
        map(
            &mut kvm.pagetable,
            KERNEL_START,
            KERNEL_START,
            end_text - KERNEL_START,
            PTE_X | PTE_R,
        )?;

        // kernel data and ram after kernel
        map(
            &mut kvm.pagetable,
            end_text,
            end_text,
            RAMEND - end_text,
            PTE_R | PTE_W,
        )?;

        // map trampoline
        map(
            &mut kvm.pagetable,
            TRAMPOLINE,
            trampoline,
            PAGESIZE,
            PTE_R | PTE_X,
        )?;

        Ok(kvm)
    }

    // maps and allocates kernel stacks
    pub fn alloc_kstack(&mut self, va: usize) {
        for i in 0..crate::process::KERNEL_STACK_PAGES {
            let kstack_page =
                FRAME_ALLOCATOR.allocate(PAGE_LAYOUT).unwrap().as_ptr() as *mut u8 as usize;
            map(
                &mut self.pagetable,
                va + i * PAGESIZE,
                kstack_page,
                PAGESIZE,
                PTE_R | PTE_W,
            )
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

pub struct Uvm {
    begin: usize,
    size: usize,
    pagetree: PageTable,
}

impl Clone for Uvm {
    fn clone(&self) -> Self {
        let mut vm = Uvm::new().unwrap();

        for addr in (USER_START..self.end()).step_by(PAGESIZE) {
            let pte = unsafe {
                self.pagetree
                    .walk(addr, WalkType::Walk)
                    .unwrap()
                    .read()
            };
            let pte = Pte::from(pte);
            if !pte.v {
                continue;
            }
            let from = pte.pa as *const u8;
            let to = FRAME_ALLOCATOR.allocate(PAGE_LAYOUT).unwrap().as_ptr() as *mut u8;
            unsafe { copy_nonoverlapping(from, to, PAGESIZE) };

            map(&mut vm.pagetree, addr, to as usize, PAGESIZE, pte.perm).unwrap();
        }

        vm
    }
}

// The address space is continuous and starts at virt 0x80000000
impl Uvm {
    pub fn new() -> Result<Uvm, ()> {
        let uvm = Uvm {
            begin: USER_START,
            size: 0,
            pagetree: PageTable::default(),
        };
        Ok(uvm)
    }

    pub fn get_satp(&self) -> SATP {
        let ppn = (self.pagetree.root.as_ptr() as usize) >> 12;
        SATP {
            mode: 1,
            asid: 0,
            ppn,
        }
    }

    pub fn free() {
        // FIXME: implement free
    }

    pub fn end(&self) -> usize {
        self.begin + self.size
    }

    pub fn grow(&mut self, size: usize, perm: usize) -> Result<(), ()> {
        self.alloc(self.size + size, perm)
    }

    // grow new pages to size
    // it creates virt address space from USERBASE to size
    pub fn alloc(&mut self, size: usize, perm: usize) -> Result<(), ()> {
        while self.size < size {
            let page = unsafe { HEAP_ALLOCATOR.alloc(PAGE_LAYOUT) as usize };
            let end = self.end();
            map(&mut self.pagetree, end, page, PAGESIZE, perm | PTE_U)?;
            // NOTE: need to free memory on fail
            self.size += PAGESIZE
        }
        Ok(())
    }

    // shrink virt address space to size
    pub fn dealloc(&mut self, size: usize) -> Result<(), ()> {
        if !size.is_multiple_of(PAGESIZE) {
            return Err(());
        }

        let newend = USER_START + size;
        unmap(&mut self.pagetree, newend, self.size - size, true)?;
        Ok(())
    }

    pub fn init_proc(&mut self, proc: &Process) -> Result<(), ()> {
        let trampoline = unsafe { &_trampoline as *const usize as usize };
        map(
            &mut self.pagetree,
            TRAMPOLINE,
            trampoline,
            PAGESIZE,
            PTE_R | PTE_X,
        )?;

        map(
            &mut self.pagetree,
            TRAPFRAME,
            proc.trapframe.as_ref() as *const Trapframe as usize,
            PAGESIZE,
            PTE_R | PTE_W,
        )?;

        Ok(())
    }

    // copy img to self at va
    // memory needs to be preallocated  TODO: does it really? Why can't it be allocated here?
    pub fn load(&mut self, mut va: usize, img: &[u8]) -> Result<(), ()> {
        // load is executed in kernel with kernel pagetree.
        if !va.is_multiple_of(PAGESIZE) {
            return Err(());
        }
        for page in img.chunks(PAGESIZE) {
            // for w in page.chunks(4) {
            //     print!("0x{:08x}\n", u32::from_le_bytes(w.try_into().unwrap()));
            // }
            let pte = unsafe {
                self.pagetree
                    .walk(va as usize, WalkType::Walk)
                    .ok_or(())?
                    .read()
            };
            let pte = Pte::from(pte);
            // NOTE: This write will go through kernel pagetree,
            // so the dst address is va in kernel virt memory,
            // but in kernel pa is identity mapped so pa = va.
            unsafe {
                let src_addr = page.as_ptr() as *const u8;
                let dst_addr = pte.pa as *mut u8;
                copy_nonoverlapping(src_addr, dst_addr, PAGESIZE);
            };
            va += PAGESIZE;
        }
        Ok(())
    }
}

// map virtual memory range to physical memory range
fn map(
    pagetree: &mut PageTable,
    virt: usize,
    phys: usize,
    size: usize,
    perm: usize,
) -> Result<(), ()> {
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
        let pte_addr = pagetree.walk(vaddr as usize, WalkType::Alloc).ok_or(())?;
        // NOTE: check for remap (I don't think it's possible)

        let mut pte = Pte::from_addr(paddr);
        pte.v = true;
        let mut pte: usize = pte.into(); // set permissions
        pte |= perm;
        // print!("-> 0x{:x} 0x{:x}\n", paddr, Pte::from(pte).pa);
        unsafe { pte_addr.write(pte) };

        vaddr += PAGESIZE;
        paddr += PAGESIZE;
    }
    Ok(())
}

// remove mappings from virt to virt+size
// if free it will also free the mapped pages but not the internal tree pages
fn unmap(pagetree: &mut PageTable, virt: usize, size: usize, free: bool) -> Result<(), ()> {
    if !size.is_multiple_of(PAGESIZE) {
        return Err(());
    }

    let mut va = virt;
    while va < virt + size {
        let pte_addr = match pagetree.walk(va as usize, WalkType::Alloc) {
            Some(x) => x,
            None => continue,
        };
        let pte = Pte::from(unsafe { pte_addr.read() });
        if !pte.v {
            continue;
        }
        if free {
            let page = (pte.ppn << 12) as *mut u8;
            unsafe { HEAP_ALLOCATOR.dealloc(page, PAGE_LAYOUT) };
        }
        unsafe { pte_addr.write(0) };
        va += PAGESIZE;
    }
    Ok(())
}

#[derive(PartialEq, Eq)]
enum WalkType {
    Alloc,
    Walk,
}

// return physical address for virual
fn walkaddr(pagetree: &mut PageTable, virt_a: usize) -> Option<usize> {
    let pte = unsafe { pagetree.walk(virt_a, WalkType::Walk).ok_or(()).ok()?.read() };
    let pa = Pte::from(pte).pa as usize + (virt_a % PAGESIZE as usize);
    Some(pa)
}

// copy from given address space INTO current
pub fn copy_in<T: Clone>(uv: &mut Uvm, addr: usize) -> Result<T, ()> {
    let user_addr = walkaddr(&mut uv.pagetree, addr).ok_or(())?;
    unsafe {
        let val = (user_addr as *const T).read();
        Ok(val)
    }
}


// Copy continuous bytes
pub fn copy_in_cont<T: Copy>(uv: &mut Uvm, addr: usize, len: usize) -> Result<Vec<T>, ()> {
    let mut bytes = Vec::new();
    let user_addr = walkaddr(&mut uv.pagetree, addr).ok_or(())?;

    for i in 0..len {
        let byte = unsafe { (user_addr as *const T).add(i).read() };
        bytes.push(byte);
    }

    Ok(bytes)
}

pub fn copy_in_str() -> Result<str, ()> {
    let s = String::new();

    Ok(s.as_str())
}

// copy from current OUT to user
pub fn copy_out<T>(uv: &mut Uvm, addr: usize, data: T) -> Result<(), ()> {
    let user_addr = walkaddr(&mut uv.pagetree, addr).ok_or(())?;
    unsafe {
        (user_addr as *mut T).write(data);
    }
    Ok(())
}

// Copy continuous bytes
pub fn copy_out_cont<T: Copy>(uv: &mut Uvm, addr: usize, data: &[T]) -> Result<(), ()> {
    let user_addr = walkaddr(&mut uv.pagetree, addr).ok_or(())?;

    for i in 0..data.len() {
        unsafe { (user_addr as *mut T).add(i).write(data[i]) };
    }

    Ok(())
}

fn _align_up(val: u32, alignment: u32) -> u32 {
    let tmp = val + alignment - 1;
    _align_down(tmp, alignment)
}

fn _align_down(val: u32, alignment: u32) -> u32 {
    let rem = val % alignment;
    val - rem
}

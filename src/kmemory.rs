use alloc::format;

use crate::{uart_print, write_csr};

unsafe extern "C" {
    pub static etext: u32;
    pub static ekernel: u32;
    pub static _STACK_PTR: u32;
}

const PAGESIZE: usize = 4 * 1024;
const RAMSIZE: usize = 62 * 1024 * 1024;
const RAMSTART: usize = 0x80000000;
const RAMEND: usize = RAMSTART + RAMSIZE;

const KERNEL_START: usize = 0x80200000;
pub const UART: usize = 0x10000000;

// struct PTPage([u32; 1024]); // 4kB page contaning 1024 PTEs

const LEVELS: u32 = 2;
const PTE_SIZE: usize = 4;

const PTE_R: u32 = 0b10;
const PTE_W: u32 = 0b100;
const PTE_X: u32 = 0b1000;

#[allow(dead_code)]
#[derive(Debug, Copy, Clone)]
struct PTE {
    pub ppn: u32,
    pub ppn1: u32,
    pub ppn0: u32,
    pub rsw: u8,
    pub d: bool,
    pub a: bool,
    pub g: bool,
    pub u: bool,
    pub x: bool,
    pub w: bool,
    pub r: bool,
    pub v: bool,
}

impl From<u32> for PTE {
    fn from(pte: u32) -> Self {
        PTE {
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
        }
    }
}

impl Into<u32> for PTE {
    fn into(self) -> u32 {
        let res = (self.ppn as u32) << 10
            | (self.rsw as u32) << 8
            | (self.d as u32) << 7
            | (self.a as u32) << 6
            | (self.g as u32) << 5
            | (self.u as u32) << 4
            | (self.x as u32) << 3
            | (self.w as u32) << 2
            | (self.r as u32) << 1
            | (self.v as u32);
        res
    }
}

#[allow(dead_code)]
impl PTE {
    #[inline]
    fn from_pa(pa: u32) -> PTE {
        let mask = (1 << 12) - 1;
        let pte = (pa & !mask) >> 2;
        PTE::from(pte)
    }

    fn set_perm(&mut self, perm: &Perm) {
        self.r = perm.r;
        self.w = perm.w;
        self.x = perm.x;
    }
}

struct Perm {
    r: bool,
    w: bool,
    x: bool,
}

impl Into<u32> for Perm {
    fn into(self) -> u32 {
        let mut res = 0;
        if self.r {
            res |= 0b10;
        }
        if self.w {
            res |= 0b100;
        }
        if self.x {
            res |= 0b1000;
        }
        res
    }
}

#[derive(Debug)]
pub struct SATP {
    mode: u32,
    asid: u32,
    ppn: u32,
}

impl Into<u32> for SATP {
    fn into(self) -> u32 {
        let mut satp: u32 = 0;
        satp |= self.mode << 31;
        satp |= self.asid << 22;
        satp |= self.ppn;
        satp
    }
}

#[derive(Debug)]
struct VA {
    vpn1: u32,
    vpn0: u32,
    offset: u32,
}

impl VA {
    fn vpn(&self, level: u32) -> Option<u32> {
        match level {
            0 => Some(self.vpn0),
            1 => Some(self.vpn1),
            _ => None,
        }
    }
}

impl From<u32> for VA {
    fn from(val: u32) -> Self {
        VA {
            vpn1: (val & 0b11111111110000000000000000000000) >> 22,
            vpn0: (val & 0b00000000001111111111000000000000) >> 12,
            offset: val & 0b00000000000000000000111111111111,
        }
    }
}

#[derive(Debug)]
struct PA {
    ppn1: u32,
    ppn0: u32,
    offset: u32,
}

impl Into<u32> for PA {
    fn into(self) -> u32 {
        let ppn1 = self.ppn1 << 22;
        let ppn0 = self.ppn0 << 12;
        ppn1 | ppn0 | self.offset
    }
}

pub struct Kmem {
    freelist: *mut u32,
}

impl Kmem {
    pub fn kalloc_init() -> Kmem {
        let kernel_end = unsafe { &ekernel as *const u32 };
        let mut kmem = Kmem {
            freelist: kernel_end as *mut u32,
        };
        let mut cursor = align_up(kernel_end as usize, PAGESIZE) as *mut u32;
        while (cursor as usize) < align_down(RAMEND, PAGESIZE) {
            // uart_print(format!("0x{:x} 0x{:x}\n", cursor as u32, RAMEND).as_str());
            kmem.kfree(cursor);
            cursor = cursor.wrapping_byte_add(PAGESIZE);
        }
        kmem
    }
    pub fn kalloc(&mut self) -> Result<*mut u32, ()> {
        // TODO: check if out of memory

        let head = self.freelist;
        self.freelist = unsafe { (self.freelist).read() as *mut u32 };
        return Ok(head);
    }
    pub fn kfree(&mut self, ptr: *mut u32) {
        // TODO: check if ptr is correct

        unsafe { ptr.write(self.freelist as u32) };
        self.freelist = ptr;
    }
}

#[derive(Default)]
pub struct Kvm {
    pagetree: *mut u32,
}

impl Kvm {
    pub fn init(memory: &mut Kmem) -> Result<Kvm, ()> {
        let root_page = memory.kalloc()?;
        let mut kvm = Kvm {
            pagetree: root_page,
        };
        // map all sections

        // uart
        kvm.kvmmap(memory, UART, UART, PAGESIZE, PTE_R | PTE_W)?;

        // kernel text
        let end_text = unsafe { &etext } as *const u32 as usize;
        kvm.kvmmap(
            memory,
            KERNEL_START,
            KERNEL_START,
            end_text - KERNEL_START,
            PTE_X | PTE_R,
        )?;

        // kernel data and ram after kernel
        kvm.kvmmap(memory, end_text, end_text, RAMEND - end_text, PTE_R | PTE_W)?;
        Ok(kvm)
    }

    pub fn start_kvm(&self) {
        let ppn = (self.pagetree as u32) >> 12;
        let satp = SATP {
            mode: 1,
            asid: 0,
            ppn,
        };
        let satp: u32 = satp.into();
        unsafe { write_csr!(satp, satp) };
    }

    // Cretae PTEs for translaition virt -> phys
    // continous virt to virt + size to continous phys to phys + size
    fn kvmmap(
        &mut self,
        memory: &mut Kmem,
        virt: usize,
        phys: usize,
        size: usize,
        perm: u32,
    ) -> Result<(), ()> {
        // TODO: tests
        // - size and virt addr aligned on page
        // - size > 0 and end < RAMEND

        let mut vaddr = virt;
        let mut paddr = phys;
        let vaddr_end = virt + size;
        while vaddr < vaddr_end {
            let pte_addr = walk(memory, self.pagetree, vaddr, true)?;
            // NOTE: check for remap (I don't think it's possible)

            let mut pte = PTE::from_pa(paddr as u32);
            pte.v = true;
            let mut pte: u32 = pte.into(); // set permissions
            pte |= perm;
            unsafe { pte_addr.write(pte) };

            vaddr += PAGESIZE;
            paddr += PAGESIZE;
        }
        Ok(())
    }
}

// returns leaf pte addr for given virtual address
// with support for megapages
fn walk(memory: &mut Kmem, pagetree: *mut u32, virt_a: usize, alloc: bool) -> Result<*mut u32, ()> {
    let va = VA::from(virt_a as u32);

    let mut a = pagetree;

    let index = va.vpn(1).ok_or(())?;
    let pte_addr = a.wrapping_add(index as usize);
    let pte_u32 = unsafe { pte_addr.read() };

    let pte = PTE::from(pte_u32);

    if pte.v {
        a = (pte.ppn << 12) as *mut u32;
    } else {
        if !alloc {
            return Err(());
        }
        let new_page_addr = memory.kalloc()? as u32;
        let mut new_pte = PTE::from_pa(new_page_addr);
        new_pte.v = true;
        unsafe { pte_addr.write(new_pte.into()) };
        a = new_page_addr as *mut u32;
    }

    let index = va.vpn(0).ok_or(())?;
    let pte_addr = a.wrapping_add(index as usize);

    Ok(pte_addr)
}

fn align_up(val: usize, alignment: usize) -> usize {
    let tmp = val + alignment - 1;
    align_down(tmp, alignment)
}

fn align_down(val: usize, alignment: usize) -> usize {
    let rem = val % alignment;
    val - rem
}

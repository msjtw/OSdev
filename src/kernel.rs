use core::alloc::GlobalAlloc;

use alloc::vec::Vec;

use crate::{
    HEAP_ALLOCATOR, KSTACK,
    process::Process,
    virtmemory::{self, PAGE_LAYOUT},
};

#[derive(Default)]
pub struct Kernel {
    pub kvm: virtmemory::Kvm,
    pub process_table: Vec<Process>,
}

impl Kernel {
    // Creates n additional processes with trapframe and kernel stack
    pub fn initproc(&mut self, n: u32) {
        let nproc = self.process_table.len() as u32;
        for i in nproc..nproc + n {
            let kstack_page = unsafe { HEAP_ALLOCATOR.alloc(PAGE_LAYOUT) as u32 };
            let proc = Process::new(i);
            self.kvm.map_kstack(kstack_page, KSTACK!(i));

            self.process_table.push(proc);
        }
    }
}

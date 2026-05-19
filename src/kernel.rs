use core::alloc::GlobalAlloc;

use alloc::vec::Vec;

use crate::{
    HEAP_ALLOCATOR, KSTACK,
    process::{self, ProcState, Process, Trapframe, forkret},
    virtmemory::{self, PAGE_LAYOUT, PAGESIZE, Uvm},
};

#[derive(Default)]
pub struct Kernel {
    pub kvm: virtmemory::Kvm,
    pub process_table: Vec<Process>,
    pub pid: u32,
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

    pub fn allocproc(&mut self) -> Option<&mut Process> {
        for p in &mut self.process_table {
            if p.state == ProcState::UNUSED {
                p.pid = Some(self.pid);
                self.pid += 1;
                p.state = ProcState::USED;
                p.trapframe = Trapframe::default();
                p.pagetable = Some(Uvm::new().unwrap());

                p.context.ra = forkret as *const u32 as u32;
                p.context.sp = p.kstack;

                return Some(p);
            }
        }

        None
    }
}

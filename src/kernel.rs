use core::alloc::GlobalAlloc;

use alloc::vec::Vec;

use crate::{
    HEAP_ALLOCATOR, KSTACK,
    process::{Context, Cpu, ProcState, Process, forkret, trapframe::Trapframe},
    virtmemory::{self, PAGE_LAYOUT, Uvm},
};

#[derive(Default)]
pub struct Kernel {
    pub kvm: virtmemory::Kvm,
    pub cpus: Cpu,
    pub process_table: Vec<Process>,
    pub pid: u32,
}

impl Kernel {
    // Creates n additional processes with trapframe and kernel stack
    pub fn initproc(&mut self, n: u32) -> Result<(), ()> {
        let nproc = self.process_table.len() as u32;
        for i in nproc..nproc + n {
            let kstack_page = unsafe { HEAP_ALLOCATOR.alloc(PAGE_LAYOUT) as u32 };
            let proc = Process::new(i)?;
            self.kvm.map_kstack(kstack_page, KSTACK!(i));

            self.process_table.push(proc);
        }
        Ok(())
    }

    pub fn allocproc(&mut self, func: fn()) -> Option<&mut Process> {
        for p in &mut self.process_table {
            if p.state == ProcState::UNUSED {
                p.pid = Some(self.pid);
                self.pid += 1;
                p.state = ProcState::RUNNABLE;
                p.trapframe = Trapframe::default();

                // get empty user page table
                let mut pagetable = Uvm::new().unwrap();
                // map trampolnie and trapframe
                pagetable.init_proc(p).unwrap();

                p.pagetable = Some(pagetable);

                p.context = Context::default();
                p.context.ra = forkret as *const u32 as u32;
                // p.context.ra = func as *const u32 as u32;
                p.context.sp = p.kstack;

                return Some(p);
            }
        }

        None
    }
}

pub mod syscall;

use alloc::{boxed::Box, vec::Vec};

use crate::{
    FRAME_ALLOCATOR, KSTACK, process::{Context, KERNEL_STACK_PAGES, ProcState, Process, forkret, trap::{interrupt_off, interrupt_on, interrupt_read, trapframe::Trapframe}}, virtmemory::{self, Kvm, PAGESIZE},
};

pub const STDIN_CHANNEL: usize = usize::MAX;



// Holds current execution state
#[derive(Default)]
pub struct Cpu {
    pub current: *mut Process,
    pub context: Context,
    pub interrupt_off_stack: usize,
    pub interrupt_prev_state: bool,
}

impl Cpu {
    pub const fn new() -> Self {
        Self {
            current: core::ptr::null_mut(),
            context: Context::zero(),
            interrupt_off_stack: 0,
            interrupt_prev_state: false,
        }
    }

    pub fn push_interrupt_off(&mut self) {
        let old = interrupt_read();
        unsafe { interrupt_off() };
        if self.interrupt_off_stack == 0 {
            self.interrupt_prev_state = old;
        }
        self.interrupt_off_stack += 1;
        // debug!("off stack: {}", self.interrupt_off_stack);
    }
    pub fn pop_interrupt_off(&mut self) {
        if self.interrupt_off_stack < 1 {
            panic!("pop interrupt; empty stack")
        }
        self.interrupt_off_stack -= 1;
        // debug!("off stack: {}", self.interrupt_off_stack);
        if self.interrupt_off_stack == 0 && self.interrupt_prev_state {
            unsafe { interrupt_on() };
        }
    }
}

#[derive(Default)]
pub struct Kernel {
    pub kvm: Option<virtmemory::Kvm>,
    pub process_table: Vec<Process>,
    pub pid: usize,
}

impl Kernel {
    pub fn init(&mut self) -> Result<(), ()> {
        self.kvm = Some(Kvm::init()?);
        Ok(())
    }

    // Creates n additional processes with trapframe and kernel stack
    pub fn initproc(&mut self, n: usize) -> Result<(), ()> {
        let nproc = self.process_table.len();
        let kvm = self.kvm.as_mut().ok_or(())?;
        for i in nproc..nproc + n {
            let proc = Process::new(i)?;
            kvm.alloc_kstack(KSTACK!(i));

            self.process_table.push(proc);
        }
        Ok(())
    }

    pub fn allocproc(&mut self) -> Option<&mut Process> {
        for p in &mut self.process_table {
            if p.state == ProcState::Unused {
                self.pid += 1;
                p.pid = Some(self.pid);
                p.state = ProcState::Used;
                p.trapframe = Box::new_in(Trapframe::default(), &FRAME_ALLOCATOR);
                unsafe { p.lock.lock_manual() };

                // get empty user page table
                // let pagetable = Uvm::new(p).unwrap();
                //
                // p.pagetable = Some(pagetable);

                p.context = Context::default();
                p.context.ra = forkret as *const () as usize;
                p.context.sp = p.kstack + KERNEL_STACK_PAGES * PAGESIZE;

                return Some(p);
            }
        }

        None
    }

    pub fn reparent(&mut self, pid: Option<usize>) {
        // let pid = pid.expect("reparent: uninitalized process");

        let mut any_child = false;
        for proc in &mut self.process_table {
            if proc.parent == pid {
                proc.parent = Some(0);
                any_child = true;
            }
        }
        if any_child {
            self.wakeup(Some(0), true);
        }
    }

    pub fn wakeup(&mut self, channel: Option<usize>, wake_all: bool) {
        unsafe {
            for proc in &mut self.process_table {
                proc.lock.lock_manual();
                if proc.state == ProcState::Sleeping && proc.sleep_channel == channel {
                    proc.state = ProcState::Runnable;
                    proc.lock.unlock_manual();
                    if !wake_all {
                        return;
                    }
                    continue;
                }
                proc.lock.unlock_manual();
            }
        }
    }
}

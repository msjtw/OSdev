use core::default;

use crate::virtmemory::{self, PTE_R, PTE_X, USER_START, Uvm};

#[derive(Debug, Copy, Clone)]
enum ProcState {
    UNUSED,
    USED,
    SLEEPING,
    RUNNABLE,
    RUNNING,
    ZOMBIE,
}

// processes are initialized on boot (state: UNUSED and kstack)
// When new process is created pid, state and pagetable are assigned.
//
#[derive(Copy, Clone)]
pub struct Process {
    pid: Option<u32>,
    state: ProcState,
    kstack: u32,                        // virt addr of kernel stack page
    pagetable: Option<virtmemory::Uvm>, // user virt pagetable
}

impl Default for Process {
    fn default() -> Self {
        Process {
            pid: None,
            state: ProcState::UNUSED,
            kstack: 0, // FIX: how to allocate kernel stack page? (global alloc ??? or constant)
            pagetable: None,
        }
    }
}

impl Process {
    fn free(&mut self) {}
}

pub fn kexec(proc: &mut Process, img: &[u8]) -> Result<(), ()> {
    let mut new_process = Process::default();
    proc.free();

    let mut pagetree = Uvm::new()?;
    pagetree.alloc(img.len() as u32, PTE_R | PTE_R | PTE_X);
    pagetree.load(USER_START, img);

    new_process.pagetable = Some(pagetree);

    *proc = new_process;
    Ok(())
}

fn load(ptree: &mut Uvm, va: u32, img: &[u8]){

}

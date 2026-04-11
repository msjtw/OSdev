use core::default;

use crate::virtmemory;

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
    pub fn new(memory: &mut virtmemory::Kmem) -> Self {
        Self {
            pid: None, // TODO: PID
            state: ProcState::UNUSED,
            kstack: page,
        }
    }
}
//

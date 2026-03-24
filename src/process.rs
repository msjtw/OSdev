use crate::virtmemory;

enum ProcState {
    UNUSED,
    USED,
    SLEEPING,
    RUNNABLE,
    RUNNING,
    ZOMBIE,
}

pub struct Process {
    pid: Option<u32>,
    state: ProcState,
    kstack: u32,
    pagetable: virtmemory::Uvm
}


// impl Process {
//     pub fn new(memory: &mut virtmemory::Kmem) -> Self {
//         Self { 
//             pid: None, // TODO: PID
//             state: ProcState::UNUSED,
//             kstack:
//             page
//         }
//     }
// }
//


use crate::{kernel::Kernel, virtmemory::{self, PTE_R, PTE_X, USER_START, Uvm}};

#[macro_export]
macro_rules! KSTACK {
    ($n:expr) => {
        virtmemory::VIRT_END - 1 - ($n + 1) * virtmemory::PAGESIZE * 2
    };
}

#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
pub enum ProcState {
    #[default]
    UNUSED,
    USED,
    SLEEPING,
    RUNNABLE,
    RUNNING,
    ZOMBIE,
}

pub const NPROC: u32 = 8;

#[derive(Clone, Copy, Default)]
struct Trapframe {
    kernel_satp: u32,
    // and others
}

// processes are initialized on boot (state: UNUSED and kstack)
// When new process is created pid, state and pagetable are assigned.
//
#[derive(Copy, Clone, Default)]
pub struct Process {
    pid: Option<u32>,
    state: ProcState,
    kstack: u32,                        // virt addr of kernel stack page
    pagetable: Option<virtmemory::Uvm>, // user virt pagetable
    trapframe: Trapframe,
}

impl Process {
    pub fn new(n: u32) -> Process {
        Process {
            kstack: KSTACK!(n),
            ..Default::default()
        }
    }


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

pub fn scheduler(kernel: &mut Kernel) -> ! {
    loop {
        for proc in &mut kernel.process_table {
            if proc.state == ProcState::RUNNABLE {
                proc.state = ProcState::RUNNING;
                // TODO: set curr cpu process as proc
                switch();
            }
        }
    }
}


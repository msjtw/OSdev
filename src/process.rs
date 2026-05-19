use crate::{
    kernel::Kernel,
    virtmemory::{self, PAGESIZE, PTE_R, PTE_W, PTE_X, USER_START, Uvm},
};

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

#[derive(Copy, Clone, Default)]
pub struct Context {
    pub ra: u32,
    pub sp: u32,

    s0: u32,
    s1: u32,
    s2: u32,
    s3: u32,
    s4: u32,
    s5: u32,
    s6: u32,
    s7: u32,
    s8: u32,
    s9: u32,
    s10: u32,
    s11: u32,
}

#[derive(Clone, Copy, Default)]
pub struct Trapframe {
    kernel_satp: u32,
    kernel_sp: u32,
    trap_handler: u32,
    epc: u32,
    hartid: u32,
    ra: u32,
    sp: u32,
    gp: u32,
    tp: u32,
    t0: u32,
    t1: u32,
    t2: u32,
    s0: u32,
    s1: u32,
    a0: u32,
    a1: u32,
    a2: u32,
    a3: u32,
    a4: u32,
    a5: u32,
    a6: u32,
    a7: u32,
    s2: u32,
    s3: u32,
    s4: u32,
    s5: u32,
    s6: u32,
    s7: u32,
    s8: u32,
    s9: u32,
    s10: u32,
    s11: u32,
    t3: u32,
    t4: u32,
    t5: u32,
    t6: u32,
}

// processes are initialized on boot (state: UNUSED and kstack)
// When new process is created pid, state and pagetable are assigned.
//
#[derive(Copy, Clone, Default)]
pub struct Process {
    pub pid: Option<u32>,
    pub state: ProcState,
    pub kstack: u32,                        // virt addr of kernel stack page
    pub pagetable: Option<virtmemory::Uvm>, // user virt pagetable
    pub context: Context,
    pub trapframe: Trapframe,
}

impl Process {
    pub fn new(n: u32) -> Process {
        Process {
            kstack: KSTACK!(n),
            ..Default::default()
        }
    }

    fn free(&mut self) {}

    pub fn kexec(&mut self, img: &[u8]) -> Result<(), ()> {
        let mut pagetree = Uvm::new()?;
        pagetree.alloc(img.len() as u32, PTE_R | PTE_W | PTE_X);
        pagetree.load(USER_START, img);

        let stack_base = pagetree.end();

        // alloc guardpage
        pagetree.alloc(PAGESIZE, 0);

        // alloc user stack
        pagetree.alloc(PAGESIZE, PTE_W | PTE_R);

        let sp = pagetree.end();

        // prepare arguments on stack
        // TODO: argc, argv
        self.trapframe.a0 = 0;

        // switch to new pagetree
        self.pagetable = Some(pagetree);
        self.trapframe.sp = sp;
        self.trapframe.epc = USER_START;

        Ok(())
    }
}

pub fn scheduler(mut kernel: Kernel) -> ! {
    loop {
        for proc in &mut kernel.process_table {
            if proc.state == ProcState::RUNNABLE {
                proc.state = ProcState::RUNNING;
                // TODO: set curr cpu process as proc
                // switch();
            }
        }
    }
}

pub fn forkret() {}

pub mod trapframe;

use alloc::format;
use core::{arch::naked_asm, mem::transmute, ptr};

use alloc::boxed::Box;

use crate::{
    CPU, FRAME_ALLOCATOR,
    allocator::FrameAllocator,
    csr::{SSTATUS_SPIE, SSTATUS_SPP},
    kernel::Kernel,
    print,
    process::trapframe::Trapframe,
    read_csr,
    trap::{
        interrupt_off, interrupt_on,
        trampoline::{_trampoline, userret, uservec},
        usertrap,
    },
    virtmemory::{self, PAGESIZE, PTE_R, PTE_W, PTE_X, TRAMPOLINE, USER_START, Uvm},
    write_csr,
};

// NOTE: AAAAAAAAAAAAAAAAAAAAAAAA
// Normaly (in c) 1 page stack for kernel is more than enough.
// But this is rust and fmt (format!) allocates shitload on stack.
pub const KERNEL_STACK_PAGES: u32 = 2;

#[macro_export]
macro_rules! KSTACK {
    ($n:expr) => {
        virtmemory::TRAMPOLINE
            - (($n + 1) * virtmemory::PAGESIZE * (crate::process::KERNEL_STACK_PAGES + 1))
            + virtmemory::PAGESIZE
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

#[repr(C)]
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

impl Context {
    pub const fn zero() -> Context {
        Context {
            ra: 0,
            sp: 0,
            s0: 0,
            s1: 0,
            s2: 0,
            s3: 0,
            s4: 0,
            s5: 0,
            s6: 0,
            s7: 0,
            s8: 0,
            s9: 0,
            s10: 0,
            s11: 0,
        }
    }
}

// processes are initialized on boot (state: UNUSED and kstack)
// When new process is created pid, state and pagetable are assigned.
//
pub struct Process {
    pub pid: Option<u32>,
    pub state: ProcState,
    pub kstack: u32,                        // virt addr of kernel stack page
    pub pagetable: Option<virtmemory::Uvm>, // user virt pagetable
    pub context: Context,
    pub trapframe: Box<Trapframe, &'static FrameAllocator>,
}

impl Process {
    pub fn new(n: u32) -> Result<Process, ()> {
        Ok(Process {
            pid: None,
            state: ProcState::default(),
            kstack: KSTACK!(n),
            pagetable: None,
            context: Context::default(),
            trapframe: Box::new_in(Trapframe::default(), &FRAME_ALLOCATOR),
        })
    }

    // fn free(&mut self) {}

    // NOTE: because yield is a keyword
    pub fn yeld(&mut self) {
        self.state = ProcState::RUNNABLE;
        unsafe { self.sched() };
    }

    unsafe fn sched(&mut self) {
        unsafe {
            let interrupt_prev_state = (*CPU).interrupt_prev_state;
            switch(&mut self.context, &mut (*CPU).context);
            (*CPU).interrupt_prev_state = interrupt_prev_state;
        }
    }

    pub fn kexec(&mut self, img: &[u8]) -> Result<(), ()> {
        let mut pagetree = Uvm::new(&self)?;
        pagetree.alloc(img.len() as u32, PTE_R | PTE_W | PTE_X)?;
        pagetree.load(USER_START, img)?;

        let _stack_base = pagetree.end();

        // alloc guardpage
        pagetree.grow(PAGESIZE, 0).unwrap();

        // alloc user stack
        pagetree.grow(PAGESIZE, PTE_W | PTE_R).unwrap();

        let sp = pagetree.end();
        print!("stack: 0x{:x}\n", sp);

        // prepare arguments on stack
        // TODO: argc, argv
        self.trapframe.a0 = 0;

        // switch to new pagetree
        self.pagetable = Some(pagetree);
        self.trapframe.sp = sp;
        // self.trapframe.epc = 0x100f;
        self.trapframe.epc = USER_START;

        Ok(())
    }
}

#[unsafe(naked)]
unsafe extern "C" fn switch(c1: &mut Context, c2: &mut Context) {
    naked_asm!(
        "
        sw ra, 0(a0)
        sw sp, 4(a0)
        sw s0, 8(a0)
        sw s1, 12(a0)
        sw s2, 16(a0)
        sw s3, 20(a0)
        sw s4, 24(a0)
        sw s5, 28(a0)
        sw s6, 32(a0)
        sw s7, 36(a0)
        sw s8, 40(a0)
        sw s9, 44(a0)
        sw s10, 48(a0)
        sw s11, 52(a0)

        lw ra, 0(a1)
        lw sp, 4(a1)
        lw s0, 8(a1)
        lw s1, 12(a1)
        lw s2, 16(a1)
        lw s3, 20(a1)
        lw s4, 24(a1)
        lw s5, 28(a1)
        lw s6, 32(a1)
        lw s7, 36(a1)
        lw s8, 40(a1)
        lw s9, 44(a1)
        lw s10, 48(a1)
        lw s11, 52(a1)
        
        ret
        "
    );
}

pub fn scheduler(mut kernel: Box<Kernel>) -> ! {
    loop {
        print!("scheduler\n");
        unsafe {
            interrupt_on();
            interrupt_off();
        }

        for proc in kernel.process_table.iter_mut() {
            if proc.state == ProcState::RUNNABLE {
                proc.state = ProcState::RUNNING;
                print!("Swiching to process {:?}\n", proc.pid);
                print!("stack pointer 0x{:x}\n", proc.trapframe.sp);
                unsafe {
                    (*CPU).current = proc as *mut Process;
                    switch(&mut (*CPU).context, &mut proc.context);
                    (*CPU).current = ptr::null_mut();
                }
            }
        }
    }
}

// allocproc sets this as ra for new processes
pub fn forkret() {
    // TODO: exec first proc (init) here (or not)
    let mut proc;
    unsafe {
        proc = &mut (*(*CPU).current);
    }

    prepare_return(&mut proc);
    let satp = proc.pagetable.unwrap().get_satp().into();
    // NOTE: userret is in 2 places, in kernel text and also mapped into
    // high address in TRAMPOLINE, we need to call it through TRAMPOLINE address.
    let userret_addr = userret as *const () as usize;
    let trampoline = unsafe { &_trampoline as *const u32 as usize };
    let userret_off = userret_addr - trampoline;
    let trampoline_userret: fn(u32) = unsafe { transmute(TRAMPOLINE as usize + userret_off) };
    trampoline_userret(satp);
}

// prepares for return to userspace
pub fn prepare_return(proc: &mut Process) {
    unsafe {
        interrupt_off();
    }

    let trampoline = unsafe { &_trampoline as *const u32 as u32 };
    let uservec_addr = uservec as *const () as u32;
    let uservec_off = uservec_addr - trampoline;
    unsafe { write_csr!(stvec, TRAMPOLINE + uservec_off) };
    // print!("uservec: 0x{:x}\n", TRAMPOLINE + uservec_off);

    // Needed for next trap into kernel
    proc.trapframe.kernel_satp = unsafe { read_csr!(satp) as u32 };
    proc.trapframe.kernel_sp = proc.kstack + KERNEL_STACK_PAGES * PAGESIZE;
    proc.trapframe.trap_handler = usertrap as *const () as u32;
    proc.trapframe.hartid = 0;

    // previous mode to user
    let mut sstatus = unsafe { read_csr!(sstatus) as u32 };
    sstatus &= !SSTATUS_SPP;
    sstatus |= SSTATUS_SPIE;
    unsafe { write_csr!(sstatus, sstatus) };

    unsafe { write_csr!(sepc, proc.trapframe.epc) };
}

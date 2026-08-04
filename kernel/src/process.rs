pub mod trapframe;

use alloc::{format, vec, vec::Vec};
use core::{arch::naked_asm, mem::transmute, ptr};

use alloc::boxed::Box;

use crate::{
    FRAME_ALLOCATOR,
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
    virtmemory::{self, PAGESIZE, PTE_R, PTE_W, PTE_X, TRAMPOLINE, USER_START, Uvm, copy_out_cont},
    write_csr,
};

// NOTE: AAAAAAAAAAAAAAAAAAAAAAAA
// Normaly (in c) 1 page stack for kernel is more than enough.
// But this is rust and fmt (format!) allocates shitload on stack.
pub const KERNEL_STACK_PAGES: usize = 2;

#[macro_export]
macro_rules! KSTACK {
    ($n:expr) => {
        virtmemory::TRAMPOLINE
            - (($n + 1) * virtmemory::PAGESIZE * ($crate::process::KERNEL_STACK_PAGES + 1))
            + virtmemory::PAGESIZE
    };
}

#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
pub enum ProcState {
    #[default]
    Unused,
    Used,
    Sleeping,
    Runnable,
    Running,
    Zombie,
}

#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct Context {
    pub ra: usize,
    pub sp: usize,

    s0: usize,
    s1: usize,
    s2: usize,
    s3: usize,
    s4: usize,
    s5: usize,
    s6: usize,
    s7: usize,
    s8: usize,
    s9: usize,
    s10: usize,
    s11: usize,
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

// processes are initialized on boot (state: Unused and kstack)
// When new process is created pid, state and pagetable are assigned.
//
pub struct Process {
    pub pid: Option<usize>,
    pub state: ProcState,
    pub kstack: usize, // virt addr of kernel stack page
    pub parent: Option<usize>,
    pub pagetable: virtmemory::Uvm, // user virt pagetable
    pub context: Context,
    pub trapframe: Box<Trapframe, &'static FrameAllocator>,
}

impl Process {
    pub fn new(n: usize) -> Result<Process, ()> {
        Ok(Process {
            pid: None,
            state: ProcState::default(),
            kstack: KSTACK!(n),
            parent: None,
            pagetable: virtmemory::Uvm::new()?,
            context: Context::default(),
            trapframe: Box::new_in(Trapframe::default(), &FRAME_ALLOCATOR),
        })
    }

    // fn free(&mut self) {}

    // NOTE: because yield is a keyword
    pub fn yeld(&mut self) {
        self.state = ProcState::Runnable;
        unsafe { self.sched() };
    }

    unsafe fn sched(&mut self) {
        unsafe {
            let interrupt_prev_state = (crate::CPU).interrupt_prev_state;
            switch(&mut self.context, &mut (crate::CPU).context);
            (crate::CPU).interrupt_prev_state = interrupt_prev_state;
        }
    }

    pub fn kfork(&mut self, kernel: &mut Kernel) -> Result<usize, ()> {
        let child_proc = kernel.allocproc().ok_or(())?;

        let mut uvm = self.pagetable.clone();
        uvm.init_proc(child_proc)?;
        child_proc.pagetable = uvm;

        child_proc.trapframe = Box::new_in(*self.trapframe.clone(), &FRAME_ALLOCATOR);

        // return 0 in child
        child_proc.trapframe.a0 = 0;
        // and cpid in parent
        self.trapframe.a0 = child_proc.pid.unwrap();

        // NOTE: not sure if it's ok
        child_proc.parent = self.pid;

        child_proc.state = ProcState::Runnable;

        child_proc.pid.ok_or(())
    }

    pub fn kexec(&mut self, img: &[u8], argv: Vec<&str>) -> Result<(), ()> {
        let mut pagetree = Uvm::new()?;
        pagetree.init_proc(self)?;
        pagetree.alloc(img.len(), PTE_R | PTE_W | PTE_X)?;
        pagetree.load(USER_START, img)?;

        // alloc guardpage
        pagetree.grow(PAGESIZE, 0).unwrap();

        // alloc user stack
        pagetree.grow(PAGESIZE, PTE_W | PTE_R).unwrap();

        let mut sp = pagetree.end() ;
        let stack_base = sp - PAGESIZE ;

        // TODO: add name as argv[0]

        // Copy args to stack
        let mut ustack = vec![];
        for arg in &argv {
            sp -= arg.len();
            sp &= !0b111; // sp is aligned to 16 bytes
            if sp < stack_base {
                return Err(());
            }
            copy_out_cont(&mut pagetree, sp, arg.as_bytes())?;
            // save addr of each arg
            ustack.push(sp);
        }
        ustack.push(0);

        // copy arg addr onto stack
        sp -= ustack.len() * size_of::<usize>(); // no need to align
        if sp < stack_base {
            return Err(());
        }
        copy_out_cont(&mut pagetree, sp, &ustack)?;

        // prepare arguments on stack
        self.trapframe.a0 = argv.len();
        self.trapframe.a1 = sp;

        // switch to new pagetree
        self.pagetable = pagetree;
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

pub fn scheduler(mut kernel: Kernel) -> ! {
    loop {
        print!("scheduler\n");
        unsafe {
            interrupt_on();
            interrupt_off();
        }

        for proc in kernel.process_table.iter_mut() {
            if proc.state == ProcState::Runnable {
                proc.state = ProcState::Running;
                print!("Swiching to process {:?}\n", proc.pid);
                print!("stack pointer 0x{:x}\n", proc.trapframe.sp);
                unsafe {
                    crate::CPU.current = proc as *mut Process;
                    switch(&mut crate::CPU.context, &mut proc.context);
                    crate::CPU.current = ptr::null_mut();
                }
            }
        }
    }
}

// allocproc sets this as ra for new processes
pub fn forkret() {
    // TODO: exec first proc (init) here (or not)
    let proc;
    unsafe {
        proc = &mut (*crate::CPU.current);
    }

    prepare_return(proc);
    let satp = proc.pagetable.get_satp().into();
    // NOTE: userret is in 2 places, in kernel text and also mapped into
    // high address in TRAMPOLINE, we need to call it through TRAMPOLINE address.
    let userret_addr = userret as *const () as usize;
    let trampoline = unsafe { &_trampoline as *const usize as usize };
    let userret_off = userret_addr - trampoline;
    let trampoline_userret: fn(usize) = unsafe { transmute(TRAMPOLINE + userret_off) };
    trampoline_userret(satp);
}

// prepares for return to userspace
pub fn prepare_return(proc: &mut Process) {
    unsafe {
        interrupt_off();
    }

    let trampoline = unsafe { &_trampoline as *const usize as usize };
    let uservec_addr = uservec as *const () as usize;
    let uservec_off = uservec_addr - trampoline;
    unsafe { write_csr!(stvec, TRAMPOLINE + uservec_off) };
    // print!("uservec: 0x{:x}\n", TRAMPOLINE + uservec_off);

    // Needed for next trap into kernel
    proc.trapframe.kernel_satp = unsafe { read_csr!(satp) };
    proc.trapframe.kernel_sp = proc.kstack + KERNEL_STACK_PAGES * PAGESIZE;
    proc.trapframe.trap_handler = usertrap as *const () as usize;
    proc.trapframe.hartid = 0;

    // previous mode to user
    let mut sstatus = unsafe { read_csr!(sstatus) as u32 };
    sstatus &= !SSTATUS_SPP;
    sstatus |= SSTATUS_SPIE;
    unsafe { write_csr!(sstatus, sstatus) };

    unsafe { write_csr!(sepc, proc.trapframe.epc) };
}

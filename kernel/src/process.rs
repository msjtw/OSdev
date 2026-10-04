mod elf;
pub mod pipe;
pub mod fd;
pub mod trapframe;

use alloc::{string::String, vec::Vec};
use core::{
    arch::{asm, naked_asm},
    mem::transmute,
    ptr,
};

use alloc::boxed::Box;

use crate::{
    FRAME_ALLOCATOR, KERNEL,
    allocator::FrameAllocator,
    csr::{SSTATUS_SPIE, SSTATUS_SPP},
    lock::IntMutex,
    println,
    process::trapframe::Trapframe,
    read_csr,
    trap::{
        interrupt_off, interrupt_on, interrupt_read,
        trampoline::{_trampoline, userret, uservec},
        usertrap,
    },
    virtmemory::{self, PAGESIZE, PTE_R, PTE_W, PTE_X, TRAMPOLINE, USER_START, Uvm, copy_out_cont},
    write_csr,
};

// NOTE: AAAAAAAAAAAAAAAAAAAAAAAA
// Normaly (in c) 1 page stack for kernel is more than enough.
// But this is rust and fmt (format!) allocates shitload on stack.
pub const KERNEL_STACK_PAGES: usize = 4;

/// A vfork wait channel is distinct from the parent-PID channels used by wait.
/// PIDs begin at one, so this can never equal the stdin channel (`usize::MAX`).
pub const fn vfork_channel(pid: usize) -> usize {
    usize::MAX - pid
}

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
    Delete,
}

#[derive(Debug, Copy, Clone)]
pub enum ForkError {
    NoProcess,
    Fault,
}

#[repr(C)]
#[derive(Copy, Clone, Default, Debug)]
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
#[derive(Debug)]
pub struct Process {
    pub id: usize,
    pub pid: Option<usize>,
    pub state: ProcState,
    pub kstack: usize, // virt addr of kernel stack page
    pub parent: Option<usize>,
    pub pagetable: virtmemory::Uvm, // user virt pagetable
    pub context: Context,
    pub xstatus: u32,
    pub sleep_channel: Option<usize>,
    pub vfork_channel: Option<usize>,
    pub trapframe: Box<Trapframe, &'static FrameAllocator>,
    pub lock: IntMutex<()>,
    pub quants: usize,
    // Linux leaves slots after close, so a table entry becomes None while its
    // index remains a valid descriptor number for later reuse.
    pub fds: fd::SharedFdTable,
}

impl Process {
    pub fn new(n: usize) -> Result<Process, ()> {
        Ok(Process {
            id: n,
            pid: None,
            state: ProcState::default(),
            kstack: KSTACK!(n),
            parent: None,
            pagetable: virtmemory::Uvm::new()?,
            context: Context::default(),
            xstatus: 0,
            sleep_channel: None,
            vfork_channel: None,
            trapframe: Box::new_in(Trapframe::default(), &FRAME_ALLOCATOR),
            lock: IntMutex::new(()),
            quants: 0,
            fds: fd::new_standard_fds(),
        })
    }

    pub fn free(&mut self) -> Result<(), ()> {
        self.pid = None;
        self.state = ProcState::Unused;
        self.parent = None;
        self.pagetable = virtmemory::Uvm::new()?;
        self.context = Context::default();
        self.xstatus = 0;
        self.sleep_channel = None;
        self.vfork_channel = None;
        self.trapframe = Box::new_in(Trapframe::default(), &FRAME_ALLOCATOR);
        self.fds = fd::new_standard_fds();

        Ok(())
    }

    // fn free(&mut self) {}

    // NOTE: because yield is a keyword
    pub fn yeld(&mut self) {
        unsafe { self.lock.lock_manual() };
        self.state = ProcState::Runnable;
        unsafe { sched(&mut self.context) };
        unsafe { self.lock.unlock_manual() };
    }

    /// Switch away from the current process after its caller has marked it
    /// Sleeping and acquired `self.lock`. The caller must release any other
    /// locks before calling this so the scheduler can run.
    pub unsafe fn sleep_locked(&mut self) {
        unsafe { sched(&mut self.context) };
        self.sleep_channel = None;
        unsafe { self.lock.unlock_manual() };
    }

    /// Wake a parent blocked by CLONE_VFORK after this process has execed or
    /// is about to exit.
    pub fn release_vfork_parent(&mut self) {
        let Some(channel) = self.vfork_channel.take() else {
            return;
        };
        KERNEL.get().unwrap().lock().wakeup(Some(channel), false);
    }

    pub fn kfork(
        &mut self,
        share_files: bool,
        vfork: bool,
        parent_tid: Option<usize>,
        child_tid: Option<usize>,
    ) -> Result<usize, ForkError> {
        let fds = if share_files {
            self.fds.clone()
        } else {
            alloc::sync::Arc::new(IntMutex::new(self.fds.lock().clone()))
        };

        let mut kernel = crate::KERNEL.get().unwrap().lock();
        let child_proc = kernel.allocproc().ok_or(ForkError::NoProcess)?;
        child_proc.trapframe = Box::new_in((*self.trapframe).clone(), &FRAME_ALLOCATOR);
        child_proc.fds = fds;
        let child_pid = child_proc.pid.ok_or(ForkError::NoProcess)?;
        child_proc.vfork_channel = vfork.then(|| vfork_channel(child_pid));

        let mut uvm = self.pagetable.clone();
        uvm.init_proc(child_proc).map_err(|()| ForkError::NoProcess)?;
        child_proc.pagetable = uvm;

        let pid_bytes = (child_pid as u32).to_ne_bytes();
        if let Some(addr) = child_tid {
            if copy_out_cont(&mut child_proc.pagetable, addr, &pid_bytes).is_err() {
                child_proc.free().ok();
                unsafe { child_proc.lock.unlock_manual() };
                return Err(ForkError::Fault);
            }
        }
        if let Some(addr) = parent_tid {
            if copy_out_cont(&mut self.pagetable, addr, &pid_bytes).is_err() {
                child_proc.free().ok();
                unsafe { child_proc.lock.unlock_manual() };
                return Err(ForkError::Fault);
            }
        }

        // return 0 in child
        child_proc.trapframe.a0 = 0;
        // and cpid in parent
        self.trapframe.a0 = child_pid;

        unsafe { child_proc.lock.unlock_manual() };
        // NOTE: not sure if it's ok
        child_proc.parent = self.pid;

        if vfork {
            // Publish the parent sleep state before the child can become
            // runnable. Keep this lock held until sys_clone schedules out.
            unsafe { self.lock.lock_manual() };
            self.sleep_channel = Some(vfork_channel(child_pid));
            self.state = ProcState::Sleeping;
        }

        unsafe { child_proc.lock.lock_manual() };
        child_proc.state = ProcState::Runnable;
        unsafe { child_proc.lock.unlock_manual() };

        Ok(child_pid)
    }

    pub fn kexec(&mut self, path: String, argv: Vec<&str>) -> Result<(), ()> {
        // TODO: when file system is implemented, load from file.

        let program = path.trim_end().rsplit('/').next().unwrap_or(path.as_str());
        let img: &[u8] = match program {
            "init" => crate::INIT,
            "prime" => crate::PRIME,
            "pipe1" => crate::PIPE1,
            "pipe2" => crate::PIPE2,
            _ => return Err(()),
        };

        let mut pagetree = Uvm::new()?;
        pagetree.init_proc(self)?;

        // ------------------------------------------------------------
        // Load ELF
        // ------------------------------------------------------------

        for segment in elf::get_elf_segments(img)? {
            if segment.p_type != elf::PT_LOAD {
                continue;
            }

            pagetree.alloc(
                segment.p_vaddr as usize,
                segment.p_memsz as usize,
                elf_flags_to_pte(segment.p_flags),
            )?;

            pagetree.load(
                segment.p_vaddr as usize,
                elf::segment_bytes(img, &segment).unwrap(),
            )?;
        }

        // ------------------------------------------------------------
        // Stack
        //
        //             high addresses
        //
        //             strings
        //             ...
        //
        //             auxv
        //             envp[]
        //             argv[]
        //             argc
        //             ^
        //             |
        //            sp
        //
        // ------------------------------------------------------------

        // Guard page.
        pagetree.alloc(pagetree.end, PAGESIZE, 0).unwrap();

        // One-page user stack.
        pagetree
            .alloc(pagetree.end, PAGESIZE, PTE_W | PTE_R)
            .unwrap();

        let stack_top = pagetree.end;
        let stack_base = stack_top - PAGESIZE;

        let mut sp = stack_top;

        // ------------------------------------------------------------
        // Build argv exactly as supplied by execve. The kernel's initial
        // process caller includes its own argv[0] as well.
        // ------------------------------------------------------------

        let args = argv;

        // ------------------------------------------------------------
        // Copy argument strings onto the stack.
        //
        // Strings grow downward.
        //
        // Every string MUST be NUL terminated.
        // ------------------------------------------------------------

        let mut arg_ptrs: Vec<usize> = Vec::with_capacity(args.len());

        for arg in args.iter().rev() {
            let bytes = arg.as_bytes();

            // +1 for terminating NUL.
            let size = bytes.len() + 1;

            if sp < stack_base + size {
                return Err(());
            }

            sp -= size;

            copy_out_cont(&mut pagetree, sp, bytes)?;

            // Write terminating '\0'.
            copy_out_cont(&mut pagetree, sp + bytes.len(), &[0])?;

            arg_ptrs.push(sp); 
        }

        // We copied arguments in reverse order.
        arg_ptrs.reverse();

        // ------------------------------------------------------------
        // Initial stack pointer must satisfy the ABI alignment.
        //
        // RISC-V requires the stack pointer to be aligned to 16 bytes
        // at a procedure-call boundary.
        //
        // Do NOT align each individual argument string.
        // Align the final stack pointer.
        // ------------------------------------------------------------

        sp &= !0xf;

        // ------------------------------------------------------------
        // Construct:
        //
        //     argc
        //     argv[0]
        //     ...
        //     argv[argc - 1]
        //     NULL
        //     envp[]
        //     NULL
        //     auxv
        //
        // We use RV32 words, so every entry is 4 bytes.
        // ------------------------------------------------------------

        let argc = arg_ptrs.len();

        // Number of words:
        //
        // argc
        // argv pointers + NULL
        // envp NULL
        // AT_NULL + value
        //
        let stack_words = 1 +                 // argc
        (argc + 1) +        // argv + NULL
        1 +                 // envp NULL
        2; // AT_NULL + 0

        let stack_bytes = stack_words * size_of::<u32>();

        if sp < stack_base + stack_bytes {
            return Err(());
        }

        sp -= stack_bytes;

        // Because we aligned sp before subtracting a multiple of 4,
        // it remains 16-byte aligned.
        // debug_assert_eq!(sp & 0xf, 0);

        let mut p = sp;

        // ------------------------------------------------------------
        // argc
        // ------------------------------------------------------------

        let argc_u32 = argc as u32;

        copy_out_cont(&mut pagetree, p, &argc_u32.to_ne_bytes())?;

        p += 4;

        // ------------------------------------------------------------
        // argv[]
        // ------------------------------------------------------------

        for &arg_ptr in &arg_ptrs {
            let ptr = arg_ptr as u32;

            copy_out_cont(&mut pagetree, p, &ptr.to_ne_bytes())?;

            p += 4;
        }

        // argv NULL terminator.
        copy_out_cont(&mut pagetree, p, &0u32.to_ne_bytes())?;

        p += 4;

        // ------------------------------------------------------------
        // envp[]
        //
        // We currently have no environment.
        //
        // envp[0] = NULL
        // ------------------------------------------------------------

        copy_out_cont(&mut pagetree, p, &0u32.to_ne_bytes())?;

        p += 4;

        // ------------------------------------------------------------
        // auxv
        //
        // Minimal valid auxiliary vector:
        //
        //     AT_NULL
        //     0
        //
        // AT_NULL is type 0.
        // ------------------------------------------------------------

        copy_out_cont(&mut pagetree, p, &0u32.to_ne_bytes())?;

        p += 4;

        copy_out_cont(&mut pagetree, p, &0u32.to_ne_bytes())?;

        // ------------------------------------------------------------
        // Enter userspace.
        //
        // The initial stack is now:
        //
        // sp -> argc
        //       argv[0]
        //       argv[1]
        //       ...
        //       NULL
        //       envp NULL
        //       AT_NULL
        //       0
        //
        // ------------------------------------------------------------

        self.pagetable = pagetree;

        self.trapframe.sp = sp;

        // Do not rely on a0/a1 for argc/argv.
        //
        // The normal process-entry ABI gets these from the initial stack.
        //
        self.trapframe.a0 = 0;
        self.trapframe.a1 = 0;

        self.trapframe.epc = USER_START;

        Ok(())
    }

    pub fn kexit(&mut self, xstatus: u32) -> ! {
        if self.pid == Some(0) {
            panic!("init exit");
        }

        self.release_vfork_parent();

        // TODO: close all open files
        {
            let lock = IntMutex::new(());
            let guard = lock.lock();
            // giveup childer to init
            KERNEL.get().unwrap().lock().reparent(self.pid);

            // wakeup parent
            KERNEL.get().unwrap().lock().wakeup(self.parent, true);

            unsafe { self.lock.lock_manual() };

            self.xstatus = xstatus;
            self.state = ProcState::Zombie;
            drop(guard);
        }

        unsafe { sched(&mut self.context) };
        panic!("cordyceps")
    }

    pub fn kwait(&mut self, nohang: bool) -> Result<Option<(usize, u32)>, ()> {
        loop {
            let parent_pid = self.pid;
            let mut has_kids = false;
            let mut zombie_pid = None;
            let mut zombie_xstatus = 0;
            let mut parent_locked_for_sleep = false;

            {
                // FIX: Hold one kernel lock across child scan and sleep-state publication
                // so wakeup cannot race between "no zombie found" and "go to sleep".
                let mut kernel = KERNEL.get().unwrap().lock();
                let table = &mut kernel.process_table;

                for proc in table.iter_mut() {
                    if proc.parent == parent_pid {
                        unsafe { proc.lock.lock_manual() };
                        has_kids = true;
                        if proc.state == ProcState::Zombie {
                            zombie_xstatus = proc.xstatus;
                            zombie_pid = proc.pid;
                            proc.free().unwrap();
                            unsafe { proc.lock.unlock_manual() };
                            break;
                        }
                        unsafe { proc.lock.unlock_manual() };
                    }
                }

                if zombie_pid.is_none() && has_kids {
                    // Hold the parent lock while publishing sleep state so wakeup() cannot
                    // race in between and be lost before we call sched().
                    let parent = table
                        .iter_mut()
                        .find(|proc| proc.pid == parent_pid)
                        .expect("waiting process missing from process table");
                    unsafe { parent.lock.lock_manual() };
                    parent.sleep_channel = parent_pid;
                    parent.state = ProcState::Sleeping;
                    parent_locked_for_sleep = true;
                }
            }

            if let Some(pid) = zombie_pid {
                return Ok(Some((pid, zombie_xstatus)));
            }

            if !has_kids {
                return Err(());
            }

            if nohang {
                return Ok(None);
            }

            if parent_locked_for_sleep {
                unsafe { sched(&mut self.context) };
                self.sleep_channel = None;
                unsafe { self.lock.unlock_manual() };
            }
        }
    }
}

fn elf_flags_to_pte(elf_flags: u32) -> usize {
    let mut pte = 0;

    if elf_flags & elf::PF_R != 0 {
        pte |= PTE_R;
    }

    if elf_flags & elf::PF_W != 0 {
        pte |= PTE_W;
    }

    if elf_flags & elf::PF_X != 0 {
        pte |= PTE_X;
    }

    pte
}

#[unsafe(naked)]
unsafe extern "C" fn switch(from: &mut Context, to: &mut Context) {
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

unsafe fn sched(context: &mut Context) {
    unsafe {
        if interrupt_read() {
            panic!("sched with interrupts enabled ")
        }

        if (crate::CPU).interrupt_off_stack != 1 {
            panic!("sched locks {}", (crate::CPU).interrupt_off_stack)
        }
        if (*crate::CPU.current).state == ProcState::Running {
            panic!("sched running")
        }

        let interrupt_prev_state = (crate::CPU).interrupt_prev_state;
        switch(context, &mut (crate::CPU).context);
        (crate::CPU).interrupt_prev_state = interrupt_prev_state;
    }
}

pub fn scheduler() -> ! {
    loop {
        // print!("scheduler: ");

        let mut found = ptr::null_mut();
        unsafe {
            interrupt_on();
            interrupt_off();
        }
        {
            let mut kernel = crate::KERNEL.get().unwrap().lock();
            let table = &mut kernel.process_table;
            let mut order: Vec<usize> = (0..table.len()).collect();

            order.sort_by_key(|&i| table[i].quants);

            for i in order {
                let proc = &mut table[i];

                unsafe { proc.lock.lock_manual() };
                if proc.state == ProcState::Runnable {
                    proc.state = ProcState::Running;
                    found = proc as *mut Process;

                    break;
                }
                unsafe { proc.lock.unlock_manual() };
            }
        }

        if !found.is_null() {
            unsafe {
                (*found).quants += 1;
                crate::CPU.current = found;
                // println!("switching to process {:?}", (*found).pid);
                switch(&mut crate::CPU.context, &mut (*found).context);
                crate::CPU.current = ptr::null_mut();
                (*found).lock.unlock_manual();
            }
        } else {
            // println!("no processes found");
            unsafe {
                interrupt_on();
                asm!("wfi");
            }
        }
    }
}

// allocproc sets this as ra for new processes
pub fn forkret() {
    let proc = unsafe { &mut (*crate::CPU.current) };

    unsafe { proc.lock.unlock_manual() };

    // TODO: exec first proc (init) here (or not)

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

pub mod trapframe;

use alloc::{string::String, vec::Vec};
use core::{
    arch::{asm, naked_asm},
    mem::transmute,
    ptr,
};

use alloc::boxed::Box;

use crate::{
    FRAME_ALLOCATOR, KERNEL, allocator::FrameAllocator, csr::{SSTATUS_SPIE, SSTATUS_SPP}, debug, kprintln, lock::{IntMutex, IntMutexGuard}, print, println, process::trapframe::Trapframe, read_csr, trap::{
        interrupt_off, interrupt_on, interrupt_read,
        trampoline::{_trampoline, userret, uservec},
        usertrap,
    }, virtmemory::{
        self, PAGESIZE, PTE_R, PTE_W, PTE_X, TRAMPOLINE, USER_START, Uvm, copy_out, copy_out_cont,
    }, write_csr
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
    Delete,
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
    pub trapframe: Box<Trapframe, &'static FrameAllocator>,
    pub lock: IntMutex<()>,
    pub quants: usize,
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
            trapframe: Box::new_in(Trapframe::default(), &FRAME_ALLOCATOR),
            lock: IntMutex::new(()),
            quants: 0,
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
        self.trapframe = Box::new_in(Trapframe::default(), &FRAME_ALLOCATOR);

        Ok(())
    }

    // fn free(&mut self) {}

    // NOTE: because yield is a keyword
    pub fn yeld(&mut self) {
        println!("interrupt: {}", interrupt_read());
        unsafe { println!("sched locks {}", (crate::CPU).interrupt_off_stack) };
        unsafe { self.lock.lock_manual() };
        println!("interrupt: {}", interrupt_read());
        unsafe { println!("sched locks {}", (crate::CPU).interrupt_off_stack) };
        self.state = ProcState::Runnable;
        unsafe { sched(&mut self.context) };
        unsafe { self.lock.unlock_manual() };
    }

    pub fn kfork(&mut self) -> Result<usize, ()> {
        let mut kernel = crate::KERNEL.get().unwrap().lock();
        let child_proc = kernel.allocproc().ok_or(())?;
        child_proc.trapframe = Box::new_in((*self.trapframe).clone(), &FRAME_ALLOCATOR);

        let mut uvm = self.pagetable.clone();
        uvm.init_proc(child_proc)?;
        child_proc.pagetable = uvm;

        // return 0 in child
        child_proc.trapframe.a0 = 0;
        // and cpid in parent
        self.trapframe.a0 = child_proc.pid.unwrap();

        unsafe { child_proc.lock.unlock_manual() };
        // NOTE: not sure if it's ok
        child_proc.parent = self.pid;

        unsafe { child_proc.lock.lock_manual() };
        child_proc.state = ProcState::Runnable;
        unsafe { child_proc.lock.unlock_manual() };

        child_proc.pid.ok_or(())
    }

    pub fn kexec(&mut self, path: String, argv: Vec<&str>) -> Result<(), ()> {
        // TODO: when file sytem is implemented load from filr

        let img: &[u8] = match path.trim_end() {
            "init" => crate::INIT,
            "prime" => crate::PRIME,
            _ => panic!("kexec: unknown program"),
        };
        let mut pagetree = Uvm::new()?;
        pagetree.init_proc(self)?;
        pagetree.alloc(img.len(), PTE_R | PTE_W | PTE_X)?;
        pagetree.load(USER_START, img)?;

        // alloc guardpage
        pagetree.grow(PAGESIZE, 0).unwrap();

        // alloc user stack
        pagetree.grow(PAGESIZE, PTE_W | PTE_R).unwrap();

        let mut sp = pagetree.end();
        let stack_base = sp - PAGESIZE;

        // TODO: add name as argv[0]

        // Copy args to stack
        let mut ustack = Vec::new();
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
        kprintln!("sp: 0x{:x}", sp);
        // self.trapframe.epc = 0x100f;
        self.trapframe.epc = USER_START;

        Ok(())
    }

    pub fn kexit(&mut self, xstatus: u32) -> ! {
        if self.pid == Some(0) {
            panic!("init exit");
        }

        // TODO: close all open files
        {
            let lock = IntMutex::new(());
            let guard = lock.lock();
            // giveup childer to init
            KERNEL.get().unwrap().lock().reparent(self.pid);

            // wakeup parent
            KERNEL.get().unwrap().lock().wakeup(self.parent);

            unsafe { self.lock.lock_manual() };

            self.xstatus = xstatus;
            self.state = ProcState::Zombie;
            drop(guard);
        }

        unsafe { sched(&mut self.context) };
        panic!("cordyceps")
    }

    pub fn kwait(&mut self, status_addr: usize) -> i32 {
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
                if status_addr != 0 {
                    copy_out(&mut self.pagetable, status_addr, zombie_xstatus).unwrap();
                }
                return pid as i32;
            }

            if !has_kids {
                println!("no kids");
                return -1;
            }

            if parent_locked_for_sleep {
                unsafe { sched(&mut self.context) };
                self.sleep_channel = None;
                unsafe { self.lock.unlock_manual() };
            }
        }
    }
    // pub fn kwait(&mut self, status_addr: usize) -> i32 {
    //     loop {
    //         let mut has_kids = false;
    //
    //         for proc in &mut KERNEL.get().unwrap().lock().process_table {
    //             if proc.parent == self.pid {
    //                 has_kids = true;
    //                 if proc.state == ProcState::Zombie {
    //                     if status_addr != 0 {
    //                         copy_out(&mut self.pagetable, status_addr, proc.xstatus).unwrap();
    //                     }
    //                     let pid = proc.pid.expect("pid-less child (what?)") as i32;
    //                     proc.free().unwrap();
    //                     return pid;
    //                 }
    //             }
    //         }
    //
    //         if !has_kids {
    //             println!("no kids");
    //             return -1;
    //         }
    //         self.sleep(self.pid);
    //     }
    // }
    //
    fn sleep(&mut self, channel: Option<usize>) {
        unsafe { self.lock.lock_manual() };
        self.sleep_channel = channel;
        self.state = ProcState::Sleeping;

        unsafe { sched(&mut self.context) };

        self.sleep_channel = None;
        unsafe { self.lock.unlock_manual() };
    }
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
        print!("scheduler: ");

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
                println!("switching to process {:?}", (*found).pid);
                switch(&mut crate::CPU.context, &mut (*found).context);
                crate::CPU.current = ptr::null_mut();
                (*found).lock.unlock_manual();
            }
        } else {
            println!("no processes found");
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

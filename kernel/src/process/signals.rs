use crate::{
    KERNEL,
    process::{
        ProcState, Process,
        fd::Errno,
        trap::trapframe::Trapframe,
    },
    virtmemory::{copy_in, copy_out},
};

pub const SIGNAL_COUNT: usize = 32;
pub const SIGINT: usize = 2;
pub const SIGKILL: usize = 9;
pub const SIGSTOP: usize = 19;
pub const SIGCHLD: usize = 17;

pub const SIG_DFL: usize = 0;
pub const SIG_IGN: usize = 1;

const SIGNAL_FRAME_MAGIC: u32 = 0x5349_4733; // "SIG3"
const SA_NODEFER: u32 = 0x4000_0000;

#[repr(C)]
#[derive(Clone, Copy)]
struct SignalFrame {
    magic: u32,
    blocked_signals: u64,
    trapframe: Trapframe,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SigSet {
    lo: u32,
    hi: u32,
}

impl From<u64> for SigSet {
    fn from(mask: u64) -> Self {
        Self {
            lo: mask as u32,
            hi: (mask >> 32) as u32,
        }
    }
}

impl From<SigSet> for u64 {
    fn from(set: SigSet) -> Self {
        set.lo as u64 | ((set.hi as u64) << 32)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SigAction {
    pub handler: usize,
    pub flags: u32,
    pub restorer: usize,
    pub mask: SigSet,
}

pub const fn signal_bit(signal: usize) -> Option<u64> {
    if signal == 0 || signal >= SIGNAL_COUNT {
        None
    } else {
        Some(1 << (signal - 1))
    }
}

pub fn send(caller: &Process, target: isize, signal: usize) -> Result<(), Errno> {
    // Signal zero is the standard existence/permission probe. SIGSTOP is not
    // implemented because this kernel has no stopped process state yet.
    if signal != 0 && (signal_bit(signal).is_none() || signal == SIGSTOP) {
        return Err(Errno::EINVAL);
    }
    if target == -1 {
        return Err(Errno::EINVAL);
    }

    let caller_pid = caller.pid.expect("process without pid");
    let caller_pgid = caller.pgid;
    let selector = match target {
        0 => Target::Group(caller_pgid.ok_or(Errno::ESRCH)?),
        1.. => Target::Process(target as usize),
        -1 => return Err(Errno::EINVAL),
        ..=-2 => Target::Group(target.checked_neg().ok_or(Errno::EINVAL)? as usize),
    };

    let mut kernel = KERNEL.get().unwrap().lock();
    let mut selected = false;
    let mut permitted = false;
    let bit = signal_bit(signal);
    for proc in &mut kernel.process_table {
        if !matches!(proc.state, ProcState::Used | ProcState::Sleeping | ProcState::Runnable | ProcState::Running)
            || !selector.matches(proc)
        {
            continue;
        }
        selected = true;
        if proc.pid != Some(caller_pid) && proc.parent != Some(caller_pid) {
            continue;
        }
        permitted = true;
        if let Some(bit) = bit {
            proc.pending_signals |= bit;
            if proc.state == ProcState::Sleeping {
                proc.state = ProcState::Runnable;
            }
        }
    }

    if !selected {
        Err(Errno::ESRCH)
    } else if !permitted {
        Err(Errno::EPERM)
    } else {
        Ok(())
    }
}

enum Target {
    Process(usize),
    Group(usize),
}

impl Target {
    fn matches(&self, proc: &Process) -> bool {
        match self {
            Self::Process(pid) => proc.pid == Some(*pid),
            Self::Group(pgid) => proc.pgid == Some(*pgid),
        }
    }
}

// mask of all signals that can be unblocked
// (because only those are implemented)
pub const fn unblockable_mask() -> u64 {
    (1 << (SIGKILL - 1)) | (1 << (SIGSTOP - 1))
}

// Does the process have any unblocked pending signals
pub fn has_unblocked_pending(proc: &Process) -> bool {
    proc.pending_signals & !(proc.blocked_signals & !unblockable_mask()) != 0
}

// deliver one pending signal
pub fn deliver_pending(proc: &mut Process) {
    let pending = proc.pending_signals & !(proc.blocked_signals & !unblockable_mask());
    if pending == 0 {
        // None pending
        return;
    }

    let signal = pending.trailing_zeros() as usize + 1;
    let bit = signal_bit(signal).unwrap();
    let action = proc.signal_actions[signal];

    if signal != SIGKILL
        && signal != SIGSTOP
        && action.handler != SIG_DFL
        && action.handler != SIG_IGN
    {
        proc.pending_signals &= !bit;
        if install_handler(proc, signal, action).is_ok() {
            return;
        }

        // A malformed handler/restorer or unusable user stack cannot be
        // recovered safely. Apply the signal's default terminating action.
        proc.term_signal = Some(signal as u32);
        proc.kexit(0);
    }

    proc.pending_signals &= !bit;
    if signal == SIGCHLD || action.handler == SIG_IGN {
        return;
    }

    proc.term_signal = Some(signal as u32);
    proc.kexit(0);
}

fn install_handler(proc: &mut Process, signal: usize, action: SigAction) -> Result<(), ()> {
    // RV32 static libc supplies a restorer trampoline which invokes
    // rt_sigreturn after the handler returns.
    if action.restorer == 0 {
        return Err(());
    }

    let frame_size = core::mem::size_of::<SignalFrame>();
    let frame_addr = proc
        .trapframe
        .sp
        .checked_sub(frame_size)
        .map(|addr| addr & !0xf)
        .ok_or(())?;
    let frame = SignalFrame {
        magic: SIGNAL_FRAME_MAGIC,
        blocked_signals: proc.blocked_signals,
        trapframe: *proc.trapframe,
    };
    copy_out(&mut proc.pagetable, frame_addr, frame)?;

    let bit = signal_bit(signal).ok_or(())?;
    proc.blocked_signals |= u64::from(action.mask);
    if action.flags & SA_NODEFER == 0 {
        proc.blocked_signals |= bit;
    }
    proc.blocked_signals &= !unblockable_mask();
    proc.trapframe.sp = frame_addr;
    proc.trapframe.epc = action.handler;
    proc.trapframe.set_return_address(action.restorer);
    proc.trapframe.a0 = signal;
    Ok(())
}

pub fn rt_sigreturn(proc: &mut Process) -> Result<(), Errno> {
    let frame = copy_in::<SignalFrame>(&mut proc.pagetable, proc.trapframe.sp)
        .map_err(|()| Errno::EFAULT)?;
    if frame.magic != SIGNAL_FRAME_MAGIC {
        return Err(Errno::EINVAL);
    }

    // Do not accept kernel trap-return state from user memory.
    let kernel_satp = proc.trapframe.kernel_satp;
    let kernel_sp = proc.trapframe.kernel_sp;
    let trap_handler = proc.trapframe.trap_handler;
    let hartid = proc.trapframe.hartid;
    *proc.trapframe = frame.trapframe;
    proc.trapframe.kernel_satp = kernel_satp;
    proc.trapframe.kernel_sp = kernel_sp;
    proc.trapframe.trap_handler = trap_handler;
    proc.trapframe.hartid = hartid;
    proc.blocked_signals = frame.blocked_signals & !unblockable_mask();
    Ok(())
}

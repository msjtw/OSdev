use crate::process::Process;

pub const SIGNAL_COUNT: usize = 32;
pub const SIGINT: usize = 2;
pub const SIGKILL: usize = 9;
pub const SIGSTOP: usize = 19;
pub const SIGCHLD: usize = 17;

pub const SIG_DFL: usize = 0;
pub const SIG_IGN: usize = 1;

/// Linux RV32 sigset_t. Keep this split into 32-bit words: user pointers are
/// only required to be 4-byte aligned on RV32.
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
        // Leave this pending until signal frames and rt_sigreturn are present.
        return;
    }

    proc.pending_signals &= !bit;
    if signal == SIGCHLD || action.handler == SIG_IGN {
        return;
    }

    // The initial implementation supports default termination. SIGCHLD is
}

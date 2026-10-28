use crate::{
    errno::Errno,
    process::{
        Process,
        signals::{self, SigAction, SigSet},
    },
    virtmemory::{copy_in, copy_out},
};

// The sigaction() system call changes the action taken by a process on receipt
// of a specific signal.
pub fn sys_rt_sigaction(proc: &mut Process) {
    const RV32_SIGSET_SIZE: usize = 8;

    let signum = proc.trapframe.a0;
    let act_addr = proc.trapframe.a1;
    let oldact_addr = proc.trapframe.a2;
    let sigsetsize = proc.trapframe.a3;

    if signals::signal_bit(signum).is_none()
        || sigsetsize != RV32_SIGSET_SIZE
        || signum == signals::SIGKILL
        || signum == signals::SIGSTOP
    {
        proc.trapframe.a0 = Errno::EINVAL.into();
        return;
    }

    let old_action = proc.signal_actions[signum];
    if oldact_addr != 0 && copy_out(&mut proc.pagetable, oldact_addr, old_action).is_err() {
        proc.trapframe.a0 = Errno::EFAULT.into();
        return;
    }
    if act_addr != 0 {
        match copy_in::<SigAction>(&mut proc.pagetable, act_addr) {
            Ok(action) => proc.signal_actions[signum] = action,
            Err(()) => {
                proc.trapframe.a0 = Errno::EFAULT.into();
                return;
            }
        }
    }
    proc.trapframe.a0 = 0;
}

pub fn sys_rt_sigprocmask(proc: &mut Process) {
    const SIG_BLOCK: usize = 0;
    const SIG_UNBLOCK: usize = 1;
    const SIG_SETMASK: usize = 2;
    const RV32_SIGSET_SIZE: usize = 8;

    let how = proc.trapframe.a0;
    let set_addr = proc.trapframe.a1;
    let oldset_addr = proc.trapframe.a2;
    let sigsetsize = proc.trapframe.a3;

    if sigsetsize != RV32_SIGSET_SIZE {
        proc.trapframe.a0 = Errno::EINVAL.into();
        return;
    }
    if oldset_addr != 0
        && copy_out(
            &mut proc.pagetable,
            oldset_addr,
            SigSet::from(proc.blocked_signals),
        )
        .is_err()
    {
        proc.trapframe.a0 = Errno::EFAULT.into();
        return;
    }
    if set_addr != 0 {
        let set = match copy_in::<SigSet>(&mut proc.pagetable, set_addr) {
            Ok(set) => u64::from(set) & !signals::unblockable_mask(),
            Err(()) => {
                proc.trapframe.a0 = Errno::EFAULT.into();
                return;
            }
        };
        match how {
            SIG_BLOCK => proc.blocked_signals |= set,
            SIG_UNBLOCK => proc.blocked_signals &= !set,
            SIG_SETMASK => proc.blocked_signals = set,
            _ => {
                proc.trapframe.a0 = Errno::EINVAL.into();
                return;
            }
        }
    }
    proc.trapframe.a0 = 0;
}

pub fn sys_rt_sigreturn(proc: &mut Process) {
    if let Err(error) = signals::rt_sigreturn(proc) {
        proc.trapframe.a0 = error.into();
    }
}

use crate::{
    process::{
        Process, ProcState,
        fd::Errno,
        signals,
    },
    read_csr64,
    virtmemory::{copy_in, copy_out},
};

const CLOCK_REALTIME: usize = 0;
const CLOCK_MONOTONIC: usize = 1;

// QEMU virt exposes a 10 MHz RISC-V time counter.
const TIMEBASE_HZ: u64 = 10_000_000;

#[repr(C)]
#[derive(Clone, Copy)]
struct Timespec64 {
    tv_sec: i64,
    tv_nsec: i64,
}

pub fn sys_clock_gettime64(proc: &mut Process) {
    let clock_id = proc.trapframe.a0;
    let timespec_addr = proc.trapframe.a1;

    if clock_id != CLOCK_REALTIME && clock_id != CLOCK_MONOTONIC {
        proc.trapframe.a0 = Errno::EINVAL.into();
        return;
    }

    let ticks = unsafe { read_csr64!(time) };
    let timespec = Timespec64 {
        tv_sec: (ticks / TIMEBASE_HZ) as i64,
        tv_nsec: ((ticks % TIMEBASE_HZ) * 1_000_000_000 / TIMEBASE_HZ) as i64,
    };

    proc.trapframe.a0 = match copy_out(&mut proc.pagetable, timespec_addr, timespec) {
        Ok(()) => 0,
        Err(()) => Errno::EFAULT.into(),
    };
}

pub fn sys_clock_getres_time64(proc: &mut Process) {
    let clock_id = proc.trapframe.a0;
    let timespec_addr = proc.trapframe.a1;

    if clock_id != CLOCK_REALTIME && clock_id != CLOCK_MONOTONIC {
        proc.trapframe.a0 = Errno::EINVAL.into();
        return;
    }

    if timespec_addr == 0 {
        proc.trapframe.a0 = 0;
        return;
    }

    let timespec = Timespec64 {
        tv_sec: 0,
        tv_nsec: (1_000_000_000 / TIMEBASE_HZ) as i64,
    };
    proc.trapframe.a0 = match copy_out(&mut proc.pagetable, timespec_addr, timespec) {
        Ok(()) => 0,
        Err(()) => Errno::EFAULT.into(),
    };
}

// Suspends the calling thread until the requested duration has elapsed or a
// signal interrupts the sleep.
pub fn sys_clock_nanosleep_time64(proc: &mut Process) {
    const TIMER_ABSTIME: usize = 1;

    let clock_id = proc.trapframe.a0;
    let flags = proc.trapframe.a1;
    let request_addr = proc.trapframe.a2;

    if (clock_id != CLOCK_REALTIME && clock_id != CLOCK_MONOTONIC) || flags & !TIMER_ABSTIME != 0 {
        proc.trapframe.a0 = Errno::EINVAL.into();
        return;
    }

    let request = match copy_in::<Timespec64>(&mut proc.pagetable, request_addr) {
        Ok(request) if request.tv_sec >= 0 && (0..1_000_000_000).contains(&request.tv_nsec) => {
            request
        }
        Ok(_) => {
            proc.trapframe.a0 = Errno::EINVAL.into();
            return;
        }
        Err(()) => {
            proc.trapframe.a0 = Errno::EFAULT.into();
            return;
        }
    };

    let request_ticks = match (request.tv_sec as u64)
        .checked_mul(TIMEBASE_HZ)
        .and_then(|ticks| ticks.checked_add(request.tv_nsec as u64 * TIMEBASE_HZ / 1_000_000_000))
    {
        Some(ticks) => ticks,
        None => {
            proc.trapframe.a0 = Errno::EINVAL.into();
            return;
        }
    };
    let now = unsafe { read_csr64!(time) };
    let wake_at = if flags & TIMER_ABSTIME != 0 {
        request_ticks
    } else {
        now.wrapping_add(request_ticks)
    };

    if now.wrapping_sub(wake_at) as i64 >= 0 {
        proc.trapframe.a0 = 0;
        return;
    }

    unsafe { proc.lock.lock_manual() };
    proc.wake_at = Some(wake_at);
    proc.state = ProcState::Sleeping;
    unsafe { proc.sleep_locked() };

    proc.wake_at = None;
    proc.trapframe.a0 = if signals::has_unblocked_pending(proc) {
        Errno::EINTR.into()
    } else {
        0
    };
}

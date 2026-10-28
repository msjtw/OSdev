use crate::{
    KERNEL,
    errno::Errno,
    read_csr64,
    virtmemory::{copy_in, copy_out_cont},
};

use super::{
    ProcState, Process,
    signals,
};

pub type PollEvents = i16;
pub const POLLIN: PollEvents = 0x001;
pub const POLLOUT: PollEvents = 0x004;
pub const POLLNVAL: PollEvents = 0x020;

static POLL_CHANNEL: u8 = 0;

pub fn poll_channel() -> usize {
    &POLL_CHANNEL as *const u8 as usize
}

pub fn wake_poll_waiters() {
    KERNEL.get().unwrap().lock().wakeup(Some(poll_channel()), true);
}

const MAX_POLL_FDS: usize = 64;
const TIMEBASE_HZ: u64 = 10_000_000;

#[repr(C)]
#[derive(Clone, Copy)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Timespec64 {
    tv_sec: i64,
    tv_nsec: i64,
}

pub fn ppoll(
    proc: &mut Process,
    fds_addr: usize,
    nfds: usize,
    timeout_addr: usize,
) -> Result<usize, Errno> {
    if nfds > MAX_POLL_FDS {
        return Err(Errno::EINVAL);
    }
    let timeout = timeout(proc, timeout_addr)?;

    loop {
        let ready = poll_once(proc, fds_addr, nfds)?;
        if ready != 0 {
            proc.wake_at = None;
            return Ok(ready);
        }
        if timeout.is_some_and(|deadline| unsafe { read_csr64!(time) }.wrapping_sub(deadline) as i64 >= 0) {
            proc.wake_at = None;
            return Ok(0);
        }
        if signals::has_unblocked_pending(proc) {
            proc.wake_at = None;
            return Err(Errno::EINTR);
        }

        unsafe { proc.lock.lock_manual() };
        proc.sleep_channel = Some(poll_channel());
        proc.wake_at = timeout;
        proc.state = ProcState::Sleeping;
        let ready = match poll_once(proc, fds_addr, nfds) {
            Ok(ready) => ready,
            Err(error) => {
                proc.state = ProcState::Runnable;
                proc.sleep_channel = None;
                proc.wake_at = None;
                unsafe { proc.lock.unlock_manual() };
                return Err(error);
            }
        };
        if ready != 0 {
            proc.state = ProcState::Runnable;
            proc.sleep_channel = None;
            proc.wake_at = None;
            unsafe { proc.lock.unlock_manual() };
            return Ok(ready);
        }
        unsafe { proc.sleep_locked() };
    }
}

fn timeout(proc: &mut Process, timeout_addr: usize) -> Result<Option<u64>, Errno> {
    if timeout_addr == 0 {
        return Ok(None);
    }
    let timeout = copy_in::<Timespec64>(&mut proc.pagetable, timeout_addr).map_err(|()| Errno::EFAULT)?;
    if timeout.tv_sec < 0 || !(0..1_000_000_000).contains(&timeout.tv_nsec) {
        return Err(Errno::EINVAL);
    }
    let ticks = (timeout.tv_sec as u64)
        .checked_mul(TIMEBASE_HZ)
        .and_then(|ticks| ticks.checked_add(timeout.tv_nsec as u64 * TIMEBASE_HZ / 1_000_000_000))
        .ok_or(Errno::EINVAL)?;
    Ok(Some(unsafe { read_csr64!(time) }.wrapping_add(ticks)))
}

fn poll_once(proc: &mut Process, fds_addr: usize, nfds: usize) -> Result<usize, Errno> {
    let mut ready = 0;
    for index in 0..nfds {
        let addr = fds_addr
            .checked_add(index * core::mem::size_of::<PollFd>())
            .ok_or(Errno::EFAULT)?;
        let mut pollfd = copy_in::<PollFd>(&mut proc.pagetable, addr).map_err(|()| Errno::EFAULT)?;
        pollfd.revents = match usize::try_from(pollfd.fd)
            .ok()
            .and_then(|fd| proc.fds.get(fd).ok())
        {
            Some(descriptor) => {
                let mut requested = pollfd.events;
                if !descriptor.readable {
                    requested &= !POLLIN;
                }
                if !descriptor.writable {
                    requested &= !POLLOUT;
                }
                descriptor.target.poll(requested)
            }
            None => POLLNVAL,
        };
        if pollfd.revents != 0 {
            ready += 1;
        }
        let bytes = unsafe {
            core::slice::from_raw_parts(
                (&pollfd as *const PollFd).cast::<u8>(),
                core::mem::size_of::<PollFd>(),
            )
        };
        copy_out_cont(&mut proc.pagetable, addr, bytes).map_err(|()| Errno::EFAULT)?;
    }
    Ok(ready)
}

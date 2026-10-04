use crate::{
    process::fd::{Errno, FileOps},
    process::Process,
    virtmemory::copy_in_bytes,
};

enum FdAccess {
    Read,
    Write,
    Any,
}

fn fd_error(error: Errno) -> usize {
    error.as_retval()
}

fn fd_ops(
    proc: &Process,
    fd: usize,
    access: FdAccess,
) -> Result<&'static dyn FileOps, Errno> {
    let entry = proc
        .fds
        .get(fd)
        .and_then(Option::as_ref)
        .ok_or(Errno::BadFileDescriptor)?;

    match access {
        FdAccess::Read if !entry.readable => Err(Errno::BadFileDescriptor),
        FdAccess::Write if !entry.writable => Err(Errno::BadFileDescriptor),
        _ => Ok(entry.ops),
    }
}

/// Linux RISC-V read for the UART-backed stdin descriptor.
pub fn sys_read(proc: &mut Process) {
    let fd = proc.trapframe.a0;
    let addr = proc.trapframe.a1;
    let size = proc.trapframe.a2;

    let ops = match fd_ops(proc, fd, FdAccess::Read) {
        Ok(ops) => ops,
        Err(error) => {
            proc.trapframe.a0 = fd_error(error);
            return;
        }
    };
    proc.trapframe.a0 = match ops.read(proc, addr, size) {
        Ok(count) => count,
        Err(error) => fd_error(error),
    };
}

pub fn sys_write(proc: &mut Process) {
    let fd = proc.trapframe.a0;
    let addr = proc.trapframe.a1;
    let size = proc.trapframe.a2;

    let ops = match fd_ops(proc, fd, FdAccess::Write) {
        Ok(ops) => ops,
        Err(error) => {
            proc.trapframe.a0 = fd_error(error);
            return;
        }
    };
    proc.trapframe.a0 = match ops.write(proc, addr, size) {
        Ok(count) => count,
        Err(error) => fd_error(error),
    };
}

/// Linux RISC-V writev for the UART-backed stdout descriptor.
pub fn sys_writev(proc: &mut Process) {
    let fd = proc.trapframe.a0;
    let mut iov_addr = proc.trapframe.a1;
    let iov_count = proc.trapframe.a2;

    let ops = match fd_ops(proc, fd, FdAccess::Write) {
        Ok(ops) => ops,
        Err(error) => {
            proc.trapframe.a0 = fd_error(error);
            return;
        }
    };
    if iov_count > 1024 {
        proc.trapframe.a0 = (-22isize) as usize; // EINVAL
        return;
    }

    let word_size = core::mem::size_of::<usize>();
    let iovec_size = word_size * 2;
    let mut written = 0usize;

    for _ in 0..iov_count {
        let record = match copy_in_bytes(&mut proc.pagetable, iov_addr, iovec_size) {
            Ok(record) => record,
            Err(()) => {
                proc.trapframe.a0 = if written == 0 {
                    (-14isize) as usize
                } else {
                    written
                };
                return;
            }
        };
        let base = usize::from_ne_bytes(record[..word_size].try_into().unwrap());
        let len = usize::from_ne_bytes(record[word_size..].try_into().unwrap());
        let mut offset = 0;
        while offset < len {
            let chunk_len = (len - offset).min(256);
            let addr = match base
                .checked_add(offset)
            {
                Some(addr) => addr,
                None => {
                    proc.trapframe.a0 = if written == 0 {
                        fd_error(Errno::Fault)
                    } else {
                        written
                    };
                    return;
                }
            };
            match ops.write(proc, addr, chunk_len) {
                Ok(count) => {
                    let Some(total) = written.checked_add(count) else {
                        proc.trapframe.a0 = fd_error(Errno::InvalidArgument);
                        return;
                    };
                    written = total;
                    if count == 0 {
                        proc.trapframe.a0 = written;
                        return;
                    }
                    offset += count;
                    if count < chunk_len {
                        proc.trapframe.a0 = written;
                        return;
                    }
                }
                Err(error) => {
                    proc.trapframe.a0 = if written == 0 { fd_error(error) } else { written };
                    return;
                }
            }
        }
        iov_addr = match iov_addr.checked_add(iovec_size) {
            Some(addr) => addr,
            None => {
                proc.trapframe.a0 = if written == 0 {
                    (-14isize) as usize
                } else {
                    written
                };
                return;
            }
        };
    }

    proc.trapframe.a0 = written;
}

/// Return a default terminal size for the UART console's TIOCGWINSZ request.
pub fn sys_ioctl(proc: &mut Process) {
    let fd = proc.trapframe.a0;
    let op = proc.trapframe.a1;
    let arg = proc.trapframe.a2;

    let ops = match fd_ops(proc, fd, FdAccess::Any) {
        Ok(ops) => ops,
        Err(error) => {
            proc.trapframe.a0 = fd_error(error);
            return;
        }
    };
    proc.trapframe.a0 = match ops.ioctl(proc, op, arg) {
        Ok(result) => result,
        Err(error) => fd_error(error),
    };
}

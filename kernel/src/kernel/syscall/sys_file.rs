use crate::{
    process::{fd::{Errno, FileDescription, FdTable, MAX_FDS}, pipe::new_pipe},
    process::Process,
    virtmemory::{copy_in_bytes, copy_out_cont},
};
use alloc::sync::Arc;

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
) -> Result<Arc<FileDescription>, Errno> {
    let fds = proc.fds.lock();
    let entry = fds
        .get(fd)
        .and_then(Option::as_ref)
        .ok_or(Errno::BadFileDescriptor)?;

    match access {
        FdAccess::Read if !entry.readable => Err(Errno::BadFileDescriptor),
        FdAccess::Write if !entry.writable => Err(Errno::BadFileDescriptor),
        _ => Ok(entry.clone()),
    }
}

fn install_fd(table: &mut FdTable, description: Arc<FileDescription>) -> Result<usize, Errno> {
    if let Some(fd) = table.iter().position(Option::is_none) {
        table[fd] = Some(description);
        return Ok(fd);
    }
    if table.len() == MAX_FDS {
        return Err(Errno::TooManyFiles);
    }

    let fd = table.len();
    table.push(Some(description));
    Ok(fd)
}

/// Linux RISC-V pipe2. Only flags == 0 is supported for now.
pub fn sys_pipe2(proc: &mut Process) {
    let pipefd_addr = proc.trapframe.a0;
    let flags = proc.trapframe.a1;

    if flags != 0 {
        proc.trapframe.a0 = fd_error(Errno::InvalidArgument);
        return;
    }

    let (read_description, write_description) = new_pipe();
    let (read_fd, write_fd) = {
        let mut fds = proc.fds.lock();
        let read_fd = match install_fd(&mut fds, read_description.clone()) {
            Ok(fd) => fd,
            Err(error) => {
                proc.trapframe.a0 = fd_error(error);
                return;
            }
        };
        let write_fd = match install_fd(&mut fds, write_description.clone()) {
            Ok(fd) => fd,
            Err(error) => {
                fds[read_fd] = None;
                proc.trapframe.a0 = fd_error(error);
                return;
            }
        };
        (read_fd, write_fd)
    };

    let mut result = [0u8; 8];
    result[0..4].copy_from_slice(&(read_fd as u32).to_ne_bytes());
    result[4..8].copy_from_slice(&(write_fd as u32).to_ne_bytes());
    if copy_out_cont(&mut proc.pagetable, pipefd_addr, &result).is_err() {
        let mut fds = proc.fds.lock();
        if let Some(Some(entry)) = fds.get(read_fd) {
            if Arc::ptr_eq(entry, &read_description) {
                fds[read_fd] = None;
            }
        }
        if let Some(Some(entry)) = fds.get(write_fd) {
            if Arc::ptr_eq(entry, &write_description) {
                fds[write_fd] = None;
            }
        }
        proc.trapframe.a0 = fd_error(Errno::Fault);
        return;
    }

    proc.trapframe.a0 = 0;
}

/// Linux RISC-V dup. The duplicate is installed in the lowest available slot.
pub fn sys_dup(proc: &mut Process) {
    let oldfd = proc.trapframe.a0;

    let mut fds = proc.fds.lock();
    let Some(entry) = fds.get(oldfd).and_then(Option::as_ref) else {
        proc.trapframe.a0 = fd_error(Errno::BadFileDescriptor);
        return;
    };
    let duplicate = entry.clone();

    if let Some(newfd) = fds.iter().position(Option::is_none) {
        fds[newfd] = Some(duplicate);
        proc.trapframe.a0 = newfd;
    } else {
        let newfd = fds.len();
        fds.push(Some(duplicate));
        proc.trapframe.a0 = newfd;
    }
}

/// Linux RISC-V dup3. `flags == 0` is the only supported mode; close-on-exec
/// state will be added with exec-time FD handling.
pub fn sys_dup3(proc: &mut Process) {
    let oldfd = proc.trapframe.a0;
    let newfd = proc.trapframe.a1;
    let flags = proc.trapframe.a2;

    if flags != 0 || oldfd == newfd {
        proc.trapframe.a0 = fd_error(Errno::InvalidArgument);
        return;
    }
    if newfd >= MAX_FDS {
        proc.trapframe.a0 = fd_error(Errno::BadFileDescriptor);
        return;
    }

    let mut fds = proc.fds.lock();
    let Some(entry) = fds.get(oldfd).and_then(Option::as_ref) else {
        proc.trapframe.a0 = fd_error(Errno::BadFileDescriptor);
        return;
    };
    let duplicate = entry.clone();

    if newfd >= fds.len() {
        fds.resize(newfd + 1, None);
    }
    fds[newfd] = Some(duplicate);
    proc.trapframe.a0 = newfd;
}

/// Linux RISC-V close. Descriptor targets are static for now, so closing only
/// clears this process's table slot.
pub fn sys_close(proc: &mut Process) {
    let fd = proc.trapframe.a0;

    let mut fds = proc.fds.lock();
    let Some(slot) = fds.get_mut(fd) else {
        proc.trapframe.a0 = fd_error(Errno::BadFileDescriptor);
        return;
    };
    if slot.is_none() {
        proc.trapframe.a0 = fd_error(Errno::BadFileDescriptor);
        return;
    }

    *slot = None;
    proc.trapframe.a0 = 0;
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
    proc.trapframe.a0 = match ops.target.read(proc, addr, size) {
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
    proc.trapframe.a0 = match ops.target.write(proc, addr, size) {
        Ok(count) => count,
        Err(error) => fd_error(error),
    };
}

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
            match ops.target.write(proc, addr, chunk_len) {
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
    proc.trapframe.a0 = match ops.target.ioctl(proc, op, arg) {
        Ok(result) => result,
        Err(error) => fd_error(error),
    };
}

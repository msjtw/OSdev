use crate::{
    process::Process,
    process::{fd::Errno, pipe::new_pipe},
    virtmemory::{copy_in, copy_out_cont},
};
use alloc::sync::Arc;

// FIX: this soudl be flags from open

/// Linux RISC-V pipe2. Only flags == 0 is supported for now.
// pub fn sys_pipe2(proc: &mut Process) {
//     let pipefd_addr = proc.trapframe.a0;
//     let flags = proc.trapframe.a1;
//
//     if flags != 0 {
//         proc.trapframe.a0 = Errno::EINVAL.into();
//         return;
//     }
//
//     let mut fds = proc.fds;
//
//     let (read_description, write_description) = new_pipe();
//     let (read_fd, write_fd) = {
//         let read_fd = match fds.insert(read_description.clone()) {
//             Ok(fd) => fd,
//             Err(error) => {
//                 proc.trapframe.a0 = error.into();
//                 return;
//             }
//         };
//         let write_fd = match fds.insert(write_description.clone()) {
//             Ok(fd) => fd,
//             Err(error) => {
//                 match fds.remove(read_fd) {
//                     Ok(_) => {}
//                     Err(error) => {
//                         proc.trapframe.a0 = error.into();
//                         return;
//                     }
//                 }
//                 proc.trapframe.a0 = error.into();
//                 return;
//             }
//         };
//         (read_fd, write_fd)
//     };
//
//     // FIX: What this even does
//     let mut result = [0u8; 8];
//     result[0..4].copy_from_slice(&(read_fd as u32).to_ne_bytes());
//     result[4..8].copy_from_slice(&(write_fd as u32).to_ne_bytes());
//     if copy_out_cont(&mut proc.pagetable, pipefd_addr, &result).is_err() {
//         if let Some(Some(entry)) = fds.get(read_fd) {
//             if Arc::ptr_eq(entry, &read_description) {
//                 fds[read_fd] = None;
//             }
//         }
//         if let Some(Some(entry)) = fds.get(write_fd) {
//             if Arc::ptr_eq(entry, &write_description) {
//                 fds[write_fd] = None;
//             }
//         }
//         proc.trapframe.a0 = Errno::EFAULT.into();
//         return;
//     }
//
//     proc.trapframe.a0 = 0;
// }

pub fn sys_dup(proc: &mut Process) {
    let fd_idx = proc.trapframe.a0;

    let fds = &mut proc.fds;

    match fds.duplicate(fd_idx, None, None) {
        Ok(dup_idx) => proc.trapframe.a0 = dup_idx,
        Err(error) => proc.trapframe.a0 = error.into(),
    }
}

// riscv doesnt have dup2

pub fn sys_dup3(proc: &mut Process) {
    let oldfd_idx = proc.trapframe.a0;
    let newfd_idx = proc.trapframe.a1;
    let flags = proc.trapframe.a2;

    let fds = &mut proc.fds;

    match fds.duplicate(oldfd_idx, Some(newfd_idx), Some(flags)) {
        Ok(dup_idx) => proc.trapframe.a0 = dup_idx,
        Err(error) => proc.trapframe.a0 = error.into(),
    }
}

pub fn sys_close(proc: &mut Process) {
    let fd_idx = proc.trapframe.a0;

    match proc.fds.remove(fd_idx) {
        Ok(_) => proc.trapframe.a0 = 0,
        Err(error) => proc.trapframe.a0 = error.into(),
    }
}

pub fn sys_read(proc: &mut Process) {
    let fd = proc.trapframe.a0;
    let addr = proc.trapframe.a1;
    let size = proc.trapframe.a2;

    let descriptor = match proc.fds.get(fd) {
        Ok(descriptor) if descriptor.readable => descriptor,
        Ok(_) => {
            proc.trapframe.a0 = Errno::EBADF.into();
            return;
        }
        Err(error) => {
            proc.trapframe.a0 = error.into();
            return;
        }
    };
    proc.trapframe.a0 = match descriptor.target.read(proc, addr, size) {
        Ok(count) => count,
        Err(error) => error.into(),
    };
}

pub fn sys_write(proc: &mut Process) {
    let fd = proc.trapframe.a0;
    let addr = proc.trapframe.a1;
    let size = proc.trapframe.a2;

    let descriptor = match proc.fds.get(fd) {
        Ok(descriptor) if descriptor.writable => descriptor,
        Ok(_) => {
            proc.trapframe.a0 = Errno::EBADF.into();
            return;
        }
        Err(error) => {
            proc.trapframe.a0 = error.into();
            return;
        }
    };
    proc.trapframe.a0 = match descriptor.target.write(proc, addr, size) {
        Ok(count) => count,
        Err(error) => error.into(),
    };
}

// The writev() system call writes iovcnt buffers of data described
// by iov to the file associated with the file descriptor fd ("gather output").
pub fn sys_writev(proc: &mut Process) {
    let fd = proc.trapframe.a0;
    let iov_base = proc.trapframe.a1;
    let iovcnt = proc.trapframe.a2;

    let descriptor = match proc.fds.get(fd) {
        Ok(descriptor) if descriptor.writable => descriptor,
        Ok(_) => {
            proc.trapframe.a0 = Errno::EBADF.into();
            return;
        }
        Err(error) => {
            proc.trapframe.a0 = error.into();
            return;
        }
    };

    // EINVAL The vector count, iovcnt, is less than zero or greater than the permitted maximum.
    if iovcnt > 1024 {
        proc.trapframe.a0 = Errno::EINVAL.into();
        return;
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Iovec {
        iov_base: usize,
        iov_len: usize,
    }

    let iovec_size = core::mem::size_of::<Iovec>();
    let mut written = 0;

    for i in 0..iovcnt {
        let iov_addr = match iov_base.checked_add(i * iovec_size) {
            Some(addr) => addr,
            None => {
                proc.trapframe.a0 = if written == 0 {
                    Errno::EFAULT.into()
                } else {
                    written
                };
                return;
            }
        };
        let iovec = match copy_in::<Iovec>(&mut proc.pagetable, iov_addr) {
            Ok(iovec) => iovec,
            Err(()) => {
                proc.trapframe.a0 = if written == 0 {
                    Errno::EFAULT.into()
                } else {
                    written
                };
                return;
            }
        };

        match descriptor
            .target
            .write(proc, iovec.iov_base, iovec.iov_len)
        {
            Ok(count) => {
                let Some(total) = written.checked_add(count) else {
                    proc.trapframe.a0 = Errno::EINVAL.into();
                    return;
                };
                written = total;
                if count < iovec.iov_len {
                    proc.trapframe.a0 = written;
                    return;
                }
            }
            Err(error) => {
                proc.trapframe.a0 = if written == 0 { error.into() } else { written };
                return;
            }
        }

    }

    proc.trapframe.a0 = written;
}

pub fn sys_ioctl(proc: &mut Process) {
    let fd = proc.trapframe.a0;
    let op = proc.trapframe.a1;
    let arg = proc.trapframe.a2;

    let descriptor = match proc.fds.get(fd) {
        Ok(descriptor) => descriptor,
        Err(error) => {
            proc.trapframe.a0 = error.into();
            return;
        }
    };
    proc.trapframe.a0 = match descriptor.target.ioctl(proc, op, arg) {
        Ok(result) => result,
        Err(error) => error.into(),
    };
}

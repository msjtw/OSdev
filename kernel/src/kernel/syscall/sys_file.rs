use crate::{
    log, process::{
        Process,
        fd::{Errno, FileDescriptor, FD_CLOEXEC, O_CLOEXEC, POLLIN, POLLNVAL, POLLOUT},
    }, vfs, virtmemory::{copy_in, copy_in_str, copy_out, copy_out_cont},
};
use alloc::sync::Arc;

pub fn sys_ppoll(proc: &mut Process) {
    const MAX_POLL_FDS: usize = 64; // only for safety

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct PollFd {
        fd: i32,
        events: i16,
        revents: i16,
    }

    let fds_addr = proc.trapframe.a0;
    let nfds = proc.trapframe.a1;
    if nfds > MAX_POLL_FDS {
        proc.trapframe.a0 = Errno::EINVAL.into();
        return;
    }

    let mut ready = 0;
    for index in 0..nfds {
        let Some(addr) = fds_addr.checked_add(index * core::mem::size_of::<PollFd>()) else {
            proc.trapframe.a0 = Errno::EFAULT.into();
            return;
        };
        let mut pollfd = match copy_in::<PollFd>(&mut proc.pagetable, addr) {
            Ok(pollfd) => pollfd,
            Err(()) => {
                proc.trapframe.a0 = Errno::EFAULT.into();
                return;
            }
        };

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
        if copy_out_cont(&mut proc.pagetable, addr, bytes).is_err() {
            proc.trapframe.a0 = Errno::EFAULT.into();
            return;
        }
    }

    proc.trapframe.a0 = ready;
}

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

pub fn sys_fcntl(proc: &mut Process) {
    const F_GETFD: usize = 1;
    const F_SETFD: usize = 2;
    const F_GETFL: usize = 3;
    const F_SETFL: usize = 4;
    const O_WRONLY: usize = 1;
    const O_RDWR: usize = 2;

    let fd = proc.trapframe.a0;
    let command = proc.trapframe.a1;
    let argument = proc.trapframe.a2;
    let descriptor = match proc.fds.get(fd) {
        Ok(descriptor) => descriptor,
        Err(error) => {
            proc.trapframe.a0 = error.into();
            return;
        }
    };

    proc.trapframe.a0 = match command {
        F_GETFD => match proc.fds.get_flags(fd) {
            Ok(flags) => flags,
            Err(error) => error.into(),
        },
        F_SETFD => match proc.fds.set_flags(fd, argument) {
            Ok(()) => 0,
            Err(error) => error.into(),
        },
        F_GETFL => match (descriptor.readable, descriptor.writable) {
            (true, true) => O_RDWR,
            (false, true) => O_WRONLY,
            _ => 0,
        },
        // The current descriptors have no mutable status flags.  Accept the
        // request so libc can configure standard streams during startup.
        F_SETFL => 0,
        _ => Errno::EINVAL.into(),
    };
}

pub fn sys_lseek(proc: &mut Process) {
    let fd = proc.trapframe.a0;
    let offset = proc.trapframe.a1 as i32 as isize;
    let whence = proc.trapframe.a2;
    proc.trapframe.a0 = match proc.fds.get(fd) {
        Ok(descriptor) => match descriptor.target.seek(offset, whence) {
            Ok(position) => position,
            Err(error) => error.into(),
        },
        Err(error) => error.into(),
    };
}

// Open a regular file from the read-only ext4 root filesystem.
pub fn sys_openat(proc: &mut Process) {
    const AT_FDCWD: usize = (-100isize) as usize;

    const O_ACCMODE: usize = 3;
    const O_RDONLY: usize = 0;
    const O_WRONLY: usize = 1;
    const O_RDWR: usize = 2;
    const O_CREAT: usize = 0x40;
    const O_TRUNC: usize = 0x200;

    let dirfd = proc.trapframe.a0;
    let path_addr = proc.trapframe.a1;
    let flags = proc.trapframe.a2;

    match copy_in_str(&mut proc.pagetable, path_addr) {
        Ok(path) => {
            log::info!(
                "openat: dirfd={} path={:?} flags={:#x} mode={:#x}",
                dirfd,
                path,
                flags,
                proc.trapframe.a3
            );
            if dirfd != AT_FDCWD || flags & (O_CREAT | O_TRUNC) != 0 {
                proc.trapframe.a0 = Errno::EROFS.into();
                return;
            }

            let path = match vfs::resolve_path(proc.cwd.as_deref().unwrap_or("/"), &path) {
                Ok(path) => path,
                Err(error) => {
                    proc.trapframe.a0 = error.into();
                    return;
                }
            };

            // TODO: currently only read only, no journaling
            if flags & O_ACCMODE != O_RDONLY {
                proc.trapframe.a0 = Errno::EROFS.into();
                return;
            }

            proc.trapframe.a0 = match vfs::open_file(&path) {
                Ok(target) => match proc.fds.push_with_flags(Arc::new(FileDescriptor {
                    target,
                    readable: true,
                    writable: false,
                }), if flags & O_CLOEXEC != 0 { FD_CLOEXEC } else { 0 }) {
                    Ok(fd) => fd,
                    Err(error) => error.into(),
                },
                Err(error) => error.into(),
            };
        }
        Err(()) => proc.trapframe.a0 = Errno::EFAULT.into(),
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct StatxTimestamp {
    tv_sec: i64,
    tv_nsec: u32,
    __reserved: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Statx {
    stx_mask: u32,
    stx_blksize: u32,
    stx_attributes: u64,
    stx_nlink: u32,
    stx_uid: u32,
    stx_gid: u32,
    stx_mode: u16,
    __spare0: u16,
    stx_ino: u64,
    stx_size: u64,
    stx_blocks: u64,
    stx_attributes_mask: u64,
    stx_atime: StatxTimestamp,
    stx_btime: StatxTimestamp,
    stx_ctime: StatxTimestamp,
    stx_mtime: StatxTimestamp,
    stx_rdev_major: u32,
    stx_rdev_minor: u32,
    stx_dev_major: u32,
    stx_dev_minor: u32,
    stx_mnt_id: u64,
    stx_dio_mem_align: u32,
    stx_dio_offset_align: u32,
    __spare3: [u64; 12],
}

// Stax size assertion
const _: [(); 256] = [(); core::mem::size_of::<Statx>()];

// Linux RISC-V statx(2). Only the mounted read-only root filesystem is
// currently supported.
pub fn sys_statx(proc: &mut Process) {
    const AT_FDCWD: usize = (-100isize) as usize;
    const STATX_BASIC_STATS: u32 = 0x07ff;
    const STATX_BTIME: u32 = 0x0800;

    let dirfd = proc.trapframe.a0;
    let path_addr = proc.trapframe.a1;
    let statx_addr = proc.trapframe.a4;

    if dirfd != AT_FDCWD {
        proc.trapframe.a0 = Errno::ENOTDIR.into();
        return;
    }

    let path = match copy_in_str(&mut proc.pagetable, path_addr) {
        Ok(path) => path,
        Err(()) => {
            proc.trapframe.a0 = Errno::EFAULT.into();
            return;
        }
    };
    let path = match vfs::resolve_path(proc.cwd.as_deref().unwrap_or("/"), &path) {
        Ok(path) => path,
        Err(error) => {
            proc.trapframe.a0 = error.into();
            return;
        }
    };

    let metadata = match vfs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) => {
            proc.trapframe.a0 = error.into();
            return;
        }
    };
    let timestamp = |seconds| StatxTimestamp {
        tv_sec: seconds as i64,
        ..Default::default()
    };
    let stat = Statx {
        stx_mask: STATX_BASIC_STATS | STATX_BTIME,
        stx_blksize: vfs::BLOCK_SIZE as u32,
        stx_nlink: metadata.links,
        stx_uid: metadata.uid,
        stx_gid: metadata.gid,
        stx_mode: metadata.mode,
        stx_ino: metadata.inode,
        stx_size: metadata.size,
        stx_blocks: metadata.blocks,
        stx_atime: timestamp(metadata.atime),
        stx_btime: timestamp(metadata.btime),
        stx_ctime: timestamp(metadata.ctime),
        stx_mtime: timestamp(metadata.mtime),
        ..Default::default()
    };

    proc.trapframe.a0 = match copy_out(&mut proc.pagetable, statx_addr, stat) {
        Ok(()) => 0,
        Err(()) => Errno::EFAULT.into(),
    };
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

// Linux RISC-V getdents64(2).
pub fn sys_getdents64(proc: &mut Process) {
    let fd = proc.trapframe.a0;
    let addr = proc.trapframe.a1;
    let len = proc.trapframe.a2;

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
    proc.trapframe.a0 = match descriptor.target.getdents64(proc, addr, len) {
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

        match descriptor.target.write(proc, iovec.iov_base, iovec.iov_len) {
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

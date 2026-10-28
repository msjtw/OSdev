#[derive(Clone, Copy, Debug)]
pub enum Errno {
    E2BIG,        // Argument list too long.
    ENOEXEC,      // Executable format error.
    ENAMETOOLONG, // File name too long.
    ENOSYS,       // Function not implemented.
    EPERM,        // Operation not permitted.
    EINTR,        // Interrupted system call.
    ESRCH,        // No such process.
    ENXIO,        // No such device or address.
    EBADF,        // Bad file descriptor.
    EFAULT,       // Bad user address.
    EINVAL,       // Invalid argument.
    ENOTTY,       // Inappropriate ioctl operation.
    ESPIPE,       // Illegal seek.
    EPIPE,        // Broken pipe.
    EMFILE,       // Too many open files.
    EAGAIN,       // Resource temporarily unavailable.
    ENOENT,       // No such file or directory.
    ENOTDIR,      // Not a directory.
    EISDIR,       // Is a directory.
    EROFS,        // Read-only filesystem.
    EIO,          // I/O error.
    EOVERFLOW,    // Value too large.
    ECHILD,       // No child processes.
    ENOMEM,       // Out of memory.
    ERANGE,       // Result out of range.
}

impl From<Errno> for usize {
    fn from(errno: Errno) -> Self {
        let code = match errno {
            Errno::EPERM => 1,
            Errno::ENOENT => 2,
            Errno::ESRCH => 3,
            Errno::EINTR => 4,
            Errno::EIO => 5,
            Errno::ENXIO => 6,
            Errno::E2BIG => 7,
            Errno::ENOEXEC => 8,
            Errno::EBADF => 9,
            Errno::ECHILD => 10,
            Errno::EAGAIN => 11,
            Errno::ENOMEM => 12,
            Errno::EFAULT => 14,
            Errno::ENOTDIR => 20,
            Errno::EISDIR => 21,
            Errno::EINVAL => 22,
            Errno::EMFILE => 24,
            Errno::ENOTTY => 25,
            Errno::ESPIPE => 29,
            Errno::EROFS => 30,
            Errno::EPIPE => 32,
            Errno::ERANGE => 34,
            Errno::ENAMETOOLONG => 36,
            Errno::ENOSYS => 38,
            Errno::EOVERFLOW => 75,
        };
        (-code as isize) as usize
    }
}

use alloc::{sync::Arc, vec::Vec};

use crate::process::Process;

#[derive(Clone, Copy, Debug)]
pub enum Errno {
    EBADF,
    EFAULT,
    EINVAL,
    ENOTTY,
    EPIPE,
    EMFILE,
    EAGAIN,
    ENOENT,
    ECHILD,
}

impl Into<usize> for Errno {
    fn into(self) -> usize {
        let code = match self {
            Self::ENOENT => 2,
            Self::EBADF => 9, // Bad file descriptor.
            Self::ECHILD => 10,
            Self::EAGAIN => 11,
            Self::EFAULT => 14, // Bad address.
            Self::EINVAL => 22, // Invalid argument.
            Self::EMFILE => 24, // Too many open files.
            Self::ENOTTY => 25, // Inappropriate ioctl for device.
            Self::EPIPE => 32,  // Broken pipe.
        };
        (-code as isize) as usize
    }
}

pub trait FileOps: core::fmt::Debug + Send + Sync {
    fn read(&self, proc: &mut Process, addr: usize, len: usize) -> Result<usize, Errno>;
    fn write(&self, proc: &mut Process, addr: usize, len: usize) -> Result<usize, Errno>;
    fn ioctl(&self, proc: &mut Process, op: usize, arg: usize) -> Result<usize, Errno>;
}

#[derive(Debug)]
pub struct FileDescriptor {
    pub target: Arc<dyn FileOps>,
    pub readable: bool,
    pub writable: bool,
}

pub const MAX_FDS: usize = 64;

#[derive(Clone, Debug)]
pub struct FdTable(Vec<Option<Arc<FileDescriptor>>>);

impl FdTable {
    pub fn new() -> Self {
        // FIX: processes shoudl share the tty
        let tty: Arc<dyn FileOps> = Arc::new(super::tty::TTY::new());

        Self(alloc::vec![
            Some(Arc::new(FileDescriptor {
                target: Arc::clone(&tty),
                readable: true,
                writable: false
            })),
            Some(Arc::new(FileDescriptor {
                target: Arc::clone(&tty),
                readable: false,
                writable: true
            })),
            Some(Arc::new(FileDescriptor {
                target: tty,
                readable: false,
                writable: true
            })),
        ])
    }

    pub fn get(&self, fd: usize) -> Result<Arc<FileDescriptor>, Errno> {
        match self.0.get(fd) {
            Some(Some(descriptor)) => Ok(Arc::clone(descriptor)),
            _ => Err(Errno::EBADF),
        }
    }

    // Insert fd into first empty fd slot
    pub fn push(&mut self, fd: Arc<FileDescriptor>) -> Result<usize, Errno> {
        if let Some(fd_idx) = self.0.iter().position(Option::is_none) {
            self.0[fd_idx] = Some(fd);
            return Ok(fd_idx);
        }
        if self.0.len() == MAX_FDS {
            // NOTE: this is only to prevent clogging memory with fds
            return Err(Errno::EMFILE);
        }

        let fd_idx = self.0.len();
        self.0.push(Some(fd));
        Ok(fd_idx)
    }

    pub fn insert(&mut self, fd: Arc<FileDescriptor>, pos: usize) -> Result<usize, Errno> {
        if pos >= MAX_FDS {
            return Err(Errno::EMFILE);
        }

        if pos < self.0.len() {
            if self.0[pos].is_some() {
                return Err(Errno::EINVAL);
            }
            self.0[pos] = Some(fd);
            return Ok(pos);
        }

        self.0.resize_with(pos + 1, || None);
        self.0[pos] = Some(fd);
        Ok(pos)
    }

    pub fn remove(&mut self, fd_idx: usize) -> Result<(), Errno> {
        let Some(fd) = self.0.get_mut(fd_idx) else {
            return Err(Errno::EBADF);
        };
        if fd.is_none() {
            return Err(Errno::EBADF);
        }

        *fd = None;
        // pop all Nones from end
        while Some(self.0.last()).is_none() {
            self.0.pop();
        }
        return Ok(());
    }

    pub fn duplicate(
        &mut self,
        oldfd_idx: usize,
        newfd_idx: Option<usize>,
        flags: Option<usize>,
    ) -> Result<usize, Errno> {
        if newfd_idx == Some(oldfd_idx) {
            if flags.is_some() {
                return Err(Errno::EINVAL);
            }
            return Ok(newfd_idx.unwrap());
        }

        if flags != Some(0) {
            // NOTE: flags are currently unsupported
            return Err(Errno::EINVAL);
        }

        let Ok(oldfd) = self.get(oldfd_idx) else {
            return Err(Errno::EBADF);
        };
        let newfd = oldfd.clone();

        let newfd_idx = match newfd_idx {
            Some(newfd_idx) => self.insert(newfd, newfd_idx)?,
            None => self.push(newfd)?,
        };

        return Ok(newfd_idx);
    }
}

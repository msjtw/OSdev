use core::fmt::Debug;

use alloc::{sync::Arc, vec::Vec};

use crate::{errno::Errno, process::Process};

use super::poll::PollEvents;

pub trait FileOps: Send + Sync {
    fn read(&self, proc: &mut Process, addr: usize, len: usize) -> Result<usize, Errno>;
    fn write(&self, proc: &mut Process, addr: usize, len: usize) -> Result<usize, Errno>;
    fn ioctl(&self, proc: &mut Process, op: usize, arg: usize) -> Result<usize, Errno>;

    fn seek(&self, _offset: isize, _whence: usize) -> Result<usize, Errno> {
        Err(Errno::ESPIPE)
    }

    // Return Linux dirent64 records for a directory descriptor.
    fn getdents64(&self, _proc: &mut Process, _addr: usize, _len: usize) -> Result<usize, Errno> {
        Err(Errno::ENOTDIR)
    }

    fn poll(&self, _events: PollEvents) -> PollEvents {
        0
    }
}

pub struct FileDescriptor {
    pub target: Arc<dyn FileOps>,
    pub readable: bool,
    pub writable: bool,
}

impl Debug for FileDescriptor {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FileDescriptor")
            .field("readable", &self.readable)
            .field("writable", &self.writable)
            .finish()
    }
}

pub const MAX_FDS: usize = 64;
pub const FD_CLOEXEC: usize = 1;

#[derive(Clone, Debug)]
struct FdEntry {
    descriptor: Arc<FileDescriptor>,
    flags: usize,
}

#[derive(Clone, Debug)]
pub struct FdTable(Vec<Option<FdEntry>>);

impl FdTable {
    pub fn new() -> Self {
        Self(alloc::vec![])
    }

    pub fn get(&self, fd: usize) -> Result<Arc<FileDescriptor>, Errno> {
        match self.0.get(fd) {
            Some(Some(entry)) => Ok(Arc::clone(&entry.descriptor)),
            _ => Err(Errno::EBADF),
        }
    }

    // Insert fd into first empty fd slot
    pub fn push(&mut self, fd: Arc<FileDescriptor>) -> Result<usize, Errno> {
        self.push_with_flags(fd, 0)
    }

    pub fn push_with_flags(
        &mut self,
        fd: Arc<FileDescriptor>,
        flags: usize,
    ) -> Result<usize, Errno> {
        if flags & !FD_CLOEXEC != 0 {
            return Err(Errno::EINVAL);
        }
        if let Some(fd_idx) = self.0.iter().position(Option::is_none) {
            self.0[fd_idx] = Some(FdEntry {
                descriptor: fd,
                flags,
            });
            return Ok(fd_idx);
        }
        if self.0.len() >= MAX_FDS {
            // NOTE: this is only to prevent clogging memory with fds
            return Err(Errno::EMFILE);
        }

        let fd_idx = self.0.len();
        self.0.push(Some(FdEntry {
            descriptor: fd,
            flags,
        }));
        Ok(fd_idx)
    }

    // push eiter both pipe ends or none
    pub fn push_pair(
        &mut self,
        first: Arc<FileDescriptor>,
        second: Arc<FileDescriptor>,
    ) -> Result<(usize, usize), Errno> {
        let available = self.0.iter().filter(|entry| entry.is_none()).count() + MAX_FDS - self.0.len();
        if available < 2 {
            return Err(Errno::EMFILE);
        }
        let first_fd = self.push(first)?;
        match self.push(second) {
            Ok(second_fd) => Ok((first_fd, second_fd)),
            Err(error) => {
                let _ = self.remove(first_fd);
                Err(error)
            }
        }
    }

    pub fn insert(&mut self, fd: Arc<FileDescriptor>, pos: usize) -> Result<usize, Errno> {
        if pos >= MAX_FDS {
            return Err(Errno::EMFILE);
        }

        if pos < self.0.len() {
            if self.0[pos].is_some() {
                return Err(Errno::EINVAL);
            }
            self.0[pos] = Some(FdEntry {
                descriptor: fd,
                flags: 0,
            });
            return Ok(pos);
        }

        self.0.resize_with(pos + 1, || None);
        self.0[pos] = Some(FdEntry {
            descriptor: fd,
            flags: 0,
        });
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
        while self.0.last().is_some_and(Option::is_none) {
            self.0.pop();
        }
        return Ok(());
    }

    pub fn duplicate(
        &mut self,
        oldfd_idx: usize,
        newfd_idx: Option<usize>,
        flags: usize,
    ) -> Result<usize, Errno> {
        if flags & !FD_CLOEXEC != 0 {
            return Err(Errno::EINVAL);
        }
        if newfd_idx == Some(oldfd_idx) {
            return Err(Errno::EINVAL);
        }

        let Ok(oldfd) = self.get(oldfd_idx) else {
            return Err(Errno::EBADF);
        };
        let newfd = oldfd.clone();

        let newfd_idx = match newfd_idx {
            Some(newfd_idx) => {
                if newfd_idx >= MAX_FDS {
                    return Err(Errno::EMFILE);
                }
                if newfd_idx >= self.0.len() {
                    self.0.resize_with(newfd_idx + 1, || None);
                }
                // dup3 atomically replaces a distinct occupied destination.
                self.0[newfd_idx] = Some(FdEntry {
                    descriptor: newfd,
                    flags,
                });
                newfd_idx
            }
            None => self.push_with_flags(newfd, flags)?,
        };

        return Ok(newfd_idx);
    }

    pub fn get_flags(&self, fd_idx: usize) -> Result<usize, Errno> {
        self.0
            .get(fd_idx)
            .and_then(Option::as_ref)
            .map(|entry| entry.flags)
            .ok_or(Errno::EBADF)
    }

    pub fn duplicate_from(
        &mut self,
        oldfd_idx: usize,
        minimum: usize,
        flags: usize,
    ) -> Result<usize, Errno> {
        if minimum >= MAX_FDS || flags & !FD_CLOEXEC != 0 {
            return Err(Errno::EINVAL);
        }
        let descriptor = self.get(oldfd_idx)?;
        let fd_idx = (minimum..MAX_FDS)
            .find(|&fd_idx| self.0.get(fd_idx).is_none_or(Option::is_none))
            .ok_or(Errno::EMFILE)?;
        if fd_idx >= self.0.len() {
            self.0.resize_with(fd_idx + 1, || None);
        }
        self.0[fd_idx] = Some(FdEntry { descriptor, flags });
        Ok(fd_idx)
    }

    pub fn set_flags(&mut self, fd_idx: usize, flags: usize) -> Result<(), Errno> {
        if flags & !FD_CLOEXEC != 0 {
            return Err(Errno::EINVAL);
        }
        let entry = self
            .0
            .get_mut(fd_idx)
            .and_then(Option::as_mut)
            .ok_or(Errno::EBADF)?;
        entry.flags = flags;
        Ok(())
    }

    pub fn close_on_exec(&mut self) {
        for entry in &mut self.0 {
            if entry
                .as_ref()
                .is_some_and(|entry| entry.flags & FD_CLOEXEC != 0)
            {
                *entry = None;
            }
        }
        while self.0.last().is_some_and(Option::is_none) {
            self.0.pop();
        }
    }
}

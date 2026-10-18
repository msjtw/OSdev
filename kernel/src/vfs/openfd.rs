use super::{LinuxDirent64, NodeKind, VfsHandle, VfsRequest, VfsResponse, request};

use crate::{
    lock::IntMutex,
    process::{
        Process,
        fd::{Errno, FileOps, PollEvents},
    },
    virtmemory::{copy_in_cont, copy_out_cont},
};

pub struct OpenFD {
    handle: VfsHandle,
    kind: NodeKind,
    position: IntMutex<usize>,
}

impl OpenFD {
    pub fn new(handle: VfsHandle, kind: NodeKind) -> Self {
        Self {
            handle,
            kind,
            position: IntMutex::new(0),
        }
    }
}

impl Drop for OpenFD {
    fn drop(&mut self) {
        let _ = request(VfsRequest::Close {
            handle: self.handle,
        });
    }
}

impl FileOps for OpenFD {
    fn seek(&self, offset: isize, whence: usize) -> Result<usize, Errno> {
        if self.kind != NodeKind::File {
            return Err(Errno::ESPIPE);
        }
        let mut position = self.position.lock();
        let base = match whence {
            0 => 0,
            1 => *position,
            2 => match request(VfsRequest::Size { handle: self.handle })? {
                VfsResponse::Size(size) => size,
                _ => return Err(Errno::EIO),
            },
            _ => return Err(Errno::EINVAL),
        };
        let next = base.checked_add_signed(offset).ok_or(Errno::EINVAL)?;
        *position = next;
        Ok(next)
    }

    fn read(&self, proc: &mut Process, addr: usize, len: usize) -> Result<usize, Errno> {
        if self.kind != NodeKind::File {
            return Err(Errno::EISDIR);
        }
        let mut position = self.position.lock();
        let VfsResponse::Data(data) = request(VfsRequest::Read {
            handle: self.handle,
            offset: *position,
            length: len,
        })?
        else {
            return Err(Errno::EIO);
        };
        copy_out_cont(&mut proc.pagetable, addr, &data).map_err(|()| Errno::EFAULT)?;
        *position = position.checked_add(data.len()).ok_or(Errno::EOVERFLOW)?;
        Ok(data.len())
    }

    fn write(&self, proc: &mut Process, addr: usize, len: usize) -> Result<usize, Errno> {
        if self.kind != NodeKind::File {
            return Err(Errno::EROFS);
        }
        let data =
            copy_in_cont::<u8>(&mut proc.pagetable, addr, len).map_err(|()| Errno::EFAULT)?;
        let position = *self.position.lock();
        let VfsResponse::Written(written) = request(VfsRequest::Write {
            handle: self.handle,
            offset: position,
            data,
        })?
        else {
            return Err(Errno::EIO);
        };
        Ok(written)
    }

    fn ioctl(&self, _proc: &mut Process, _op: usize, _arg: usize) -> Result<usize, Errno> {
        Err(Errno::ENOTTY)
    }

    fn poll(&self, events: PollEvents) -> PollEvents {
        events
    }

    fn getdents64(&self, proc: &mut Process, addr: usize, len: usize) -> Result<usize, Errno> {
        if self.kind != NodeKind::Directory {
            return Err(Errno::ENOTDIR);
        }
        let mut position = self.position.lock();
        let mut written = 0;
        loop {
            let VfsResponse::DirectoryEntry(entry) = request(VfsRequest::ReadDir {
                handle: self.handle,
                position: *position,
            })?
            else {
                return Err(Errno::EIO);
            };
            let Some(entry) = entry else { break };
            let record = LinuxDirent64 {
                inode: entry.inode,
                next_offset: *position as i64 + 1,
                file_type: entry.file_type,
                name: &entry.name,
            };
            let bytes = record.encode();
            if bytes.len() > len - written {
                break;
            }
            copy_out_cont(&mut proc.pagetable, addr + written, &bytes)
                .map_err(|()| Errno::EFAULT)?;
            written += bytes.len();
            *position += 1;
        }
        Ok(written)
    }
}

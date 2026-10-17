use alloc::{sync::Arc, vec::Vec};

use crate::{
    process::{
        Process,
        fd::{Errno, FileOps, POLLIN, POLLOUT, PollEvents},
        tty::TTY,
    },
    virtmemory::{copy_in_cont, copy_out_cont},
};

use super::{AccessMode, DirectoryEntry, FileMetadata, NodeKind, VfsNode};

const DEV_INODE: u64 = 0xffff_0000;
const CONSOLE_INODE: u64 = DEV_INODE + 1;
const TTY_INODE: u64 = DEV_INODE + 2;
const NULL_INODE: u64 = DEV_INODE + 3;
const ZERO_INODE: u64 = DEV_INODE + 4;
const DT_CHR: u8 = 2;
const DT_DIR: u8 = 4;

#[derive(Clone, Copy)]
enum Device {
    Console,
    Tty,
    Null,
    Zero,
}

fn device(path: &str) -> Option<Device> {
    match path {
        "/dev/console" => Some(Device::Console),
        "/dev/tty" => Some(Device::Tty),
        "/dev/null" => Some(Device::Null),
        "/dev/zero" => Some(Device::Zero),
        _ => None,
    }
}

pub(super) fn is_device(path: &str) -> bool {
    device(path).is_some()
}

fn metadata(inode: u64, mode: u16) -> FileMetadata {
    FileMetadata {
        inode,
        mode,
        uid: 0,
        gid: 0,
        links: 1,
        size: 0,
        blocks: 0,
        atime: 0,
        mtime: 0,
        ctime: 0,
        btime: 0,
    }
}

pub(super) fn node(path: &str) -> Option<Result<VfsNode, Errno>> {
    if path == "/dev" {
        return Some(Ok(VfsNode {
            metadata: metadata(DEV_INODE, 0o040755),
            kind: NodeKind::Directory,
        }));
    }
    if !path.starts_with("/dev/") {
        return None;
    }
    let device = device(path).ok_or(Errno::ENOENT);
    Some(device.map(|device| {
        let inode = match device {
            Device::Console => CONSOLE_INODE,
            Device::Tty => TTY_INODE,
            Device::Null => NULL_INODE,
            Device::Zero => ZERO_INODE,
        };
        VfsNode {
            metadata: metadata(inode, 0o020666),
            kind: NodeKind::File,
        }
    }))
}

pub(super) fn directory_entries() -> Vec<DirectoryEntry> {
    [
        (CONSOLE_INODE, b"console".as_slice()),
        (TTY_INODE, b"tty".as_slice()),
        (NULL_INODE, b"null".as_slice()),
        (ZERO_INODE, b"zero".as_slice()),
    ]
    .into_iter()
    .map(|(inode, name)| DirectoryEntry {
        inode,
        file_type: DT_CHR,
        name: name.to_vec(),
    })
    .collect()
}

pub(super) fn root_entry() -> DirectoryEntry {
    DirectoryEntry {
        inode: DEV_INODE,
        file_type: DT_DIR,
        name: b"dev".to_vec(),
    }
}

pub(super) fn open(proc: &Process, path: &str, _access: AccessMode) -> Option<Result<Arc<dyn FileOps>, Errno>> {
    let device = device(path)?;
    Some(match device {
        Device::Console => Ok(Arc::new(DeviceFile::Console(TTY::get()))),
        Device::Tty if TTY::is_controlling_terminal(proc) => Ok(Arc::new(DeviceFile::Console(TTY::get()))),
        Device::Tty => Err(Errno::ENXIO),
        Device::Null => Ok(Arc::new(DeviceFile::Null)),
        Device::Zero => Ok(Arc::new(DeviceFile::Zero)),
    })
}

enum DeviceFile {
    Console(Arc<TTY>),
    Null,
    Zero,
}

impl FileOps for DeviceFile {
    fn read(&self, proc: &mut Process, addr: usize, len: usize) -> Result<usize, Errno> {
        match self {
            Self::Console(tty) => tty.read(proc, addr, len),
            Self::Null => Ok(0),
            Self::Zero => {
                const ZEROES: [u8; 256] = [0; 256];
                let mut copied = 0;
                while copied < len {
                    let count = (len - copied).min(ZEROES.len());
                    let target = addr.checked_add(copied).ok_or(Errno::EFAULT)?;
                    copy_out_cont(&mut proc.pagetable, target, &ZEROES[..count])
                        .map_err(|()| Errno::EFAULT)?;
                    copied += count;
                }
                Ok(copied)
            }
        }
    }

    fn write(&self, proc: &mut Process, addr: usize, len: usize) -> Result<usize, Errno> {
        match self {
            Self::Console(tty) => tty.write(proc, addr, len),
            Self::Null | Self::Zero => {
                let mut copied = 0;
                while copied < len {
                    let count = (len - copied).min(256);
                    let source = addr.checked_add(copied).ok_or(Errno::EFAULT)?;
                    copy_in_cont::<u8>(&mut proc.pagetable, source, count)
                        .map_err(|()| Errno::EFAULT)?;
                    copied += count;
                }
                Ok(copied)
            }
        }
    }

    fn ioctl(&self, proc: &mut Process, op: usize, arg: usize) -> Result<usize, Errno> {
        match self {
            Self::Console(tty) => tty.ioctl(proc, op, arg),
            Self::Null | Self::Zero => Err(Errno::ENOTTY),
        }
    }

    fn poll(&self, events: PollEvents) -> PollEvents {
        match self {
            Self::Console(tty) => tty.poll(events),
            Self::Null | Self::Zero => events & (POLLIN | POLLOUT),
        }
    }
}

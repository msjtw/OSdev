use core::fmt::Write;

use alloc::{string::String, sync::Arc, vec::Vec};

use crate::{
    HEAP_ALLOCATOR,
    errno::Errno,
    lock::IntMutex,
    process::{
        Process,
        fd::FileOps,
        poll::{POLLIN, PollEvents},
    },
    read_csr64,
    virtmemory::copy_out_cont,
};

use super::{AccessMode, DirectoryEntry, FileMetadata, NodeKind, VfsNode};

const PROC_INODE: u64 = 0xffff_1000;
const UPTIME_INODE: u64 = PROC_INODE + 1;
const MEMINFO_INODE: u64 = PROC_INODE + 2;
const VERSION_INODE: u64 = PROC_INODE + 3;
const MOUNTS_INODE: u64 = PROC_INODE + 4;
const DT_DIR: u8 = 4;
const DT_REG: u8 = 8;
const TIMEBASE_HZ: u64 = 10_000_000;

#[derive(Clone, Copy)]
enum ProcFile {
    Uptime,
    Meminfo,
    Version,
    Mounts,
}

fn file(path: &str) -> Option<ProcFile> {
    match path {
        "/proc/uptime" => Some(ProcFile::Uptime),
        "/proc/meminfo" => Some(ProcFile::Meminfo),
        "/proc/version" => Some(ProcFile::Version),
        "/proc/mounts" => Some(ProcFile::Mounts),
        _ => None,
    }
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
    if path == "/proc" {
        return Some(Ok(VfsNode {
            metadata: metadata(PROC_INODE, 0o040555),
            kind: NodeKind::Directory,
        }));
    }
    if !path.starts_with("/proc/") {
        return None;
    }
    Some(file(path).ok_or(Errno::ENOENT).map(|file| {
        let inode = match file {
            ProcFile::Uptime => UPTIME_INODE,
            ProcFile::Meminfo => MEMINFO_INODE,
            ProcFile::Version => VERSION_INODE,
            ProcFile::Mounts => MOUNTS_INODE,
        };
        VfsNode {
            metadata: metadata(inode, 0o100444),
            kind: NodeKind::File,
        }
    }))
}

pub(super) fn directory_entries() -> Vec<DirectoryEntry> {
    [
        (UPTIME_INODE, b"uptime".as_slice()),
        (MEMINFO_INODE, b"meminfo".as_slice()),
        (VERSION_INODE, b"version".as_slice()),
        (MOUNTS_INODE, b"mounts".as_slice()),
    ]
    .into_iter()
    .map(|(inode, name)| DirectoryEntry {
        inode,
        file_type: DT_REG,
        name: name.to_vec(),
    })
    .collect()
}

pub(super) fn root_entry() -> DirectoryEntry {
    DirectoryEntry {
        inode: PROC_INODE,
        file_type: DT_DIR,
        name: b"proc".to_vec(),
    }
}

pub(super) fn open(path: &str, access: AccessMode) -> Option<Result<Arc<dyn FileOps>, Errno>> {
    let file = file(path)?;
    Some(if matches!(access, AccessMode::ReadOnly) {
        Ok(Arc::new(ProcOpen::new(contents(file))))
    } else {
        Err(Errno::EROFS)
    })
}

pub(super) fn read_all(path: &str) -> Option<Result<Vec<u8>, Errno>> {
    file(path).map(|file| Ok(contents(file)))
}

fn contents(file: ProcFile) -> Vec<u8> {
    match file {
        ProcFile::Uptime => {
            let ticks = unsafe { read_csr64!(time) };
            let mut text = String::new();
            let _ = writeln!(text, "{}.{:02} 0.00", ticks / TIMEBASE_HZ, (ticks % TIMEBASE_HZ) * 100 / TIMEBASE_HZ);
            text.into_bytes()
        }
        ProcFile::Meminfo => {
            let (total, used) = {
                let heap = HEAP_ALLOCATOR.lock();
                (heap.stats_total_bytes() / 1024, heap.stats_alloc_actual() / 1024)
            };
            let mut text = String::new();
            let _ = writeln!(text, "MemTotal:       {total:>8} kB");
            let _ = writeln!(text, "MemFree:        {:>8} kB", total.saturating_sub(used));
            let _ = writeln!(text, "MemAvailable:   {:>8} kB", total.saturating_sub(used));
            text.into_bytes()
        }
        ProcFile::Version => b"rust-baremetal rv32 kernel\n".to_vec(),
        ProcFile::Mounts => b"rootfs / ext4 ro 0 0\ndevfs /dev devfs rw 0 0\nprocfs /proc proc ro 0 0\n".to_vec(),
    }
}

struct ProcOpen {
    data: Vec<u8>,
    position: IntMutex<usize>,
}

impl ProcOpen {
    fn new(data: Vec<u8>) -> Self {
        Self { data, position: IntMutex::new(0) }
    }
}

impl FileOps for ProcOpen {
    fn read(&self, proc: &mut Process, addr: usize, len: usize) -> Result<usize, Errno> {
        let mut position = self.position.lock();
        let available = self.data.get(*position..).unwrap_or_default();
        let count = len.min(available.len());
        copy_out_cont(&mut proc.pagetable, addr, &available[..count]).map_err(|()| Errno::EFAULT)?;
        *position += count;
        Ok(count)
    }

    fn write(&self, _proc: &mut Process, _addr: usize, _len: usize) -> Result<usize, Errno> {
        Err(Errno::EROFS)
    }

    fn ioctl(&self, _proc: &mut Process, _op: usize, _arg: usize) -> Result<usize, Errno> {
        Err(Errno::ENOTTY)
    }

    fn seek(&self, offset: isize, whence: usize) -> Result<usize, Errno> {
        let mut position = self.position.lock();
        let base = match whence {
            0 => 0,
            1 => *position,
            2 => self.data.len(),
            _ => return Err(Errno::EINVAL),
        };
        let next = base.checked_add_signed(offset).ok_or(Errno::EINVAL)?;
        *position = next;
        Ok(next)
    }

    fn poll(&self, events: PollEvents) -> PollEvents {
        events & POLLIN
    }
}

use alloc::{sync::Arc, vec, vec::Vec};
use core::cmp::min;

use ext4_rs::{BLOCK_SIZE, BlockDevice as Ext4BlockDevice, Ext4};
use spin::Once;

use crate::{
    drivers::block::{self, BlockDevice, SharedBlockDevice},
    lock::IntMutex,
    log,
    process::{
        Process,
        fd::{Errno, FileOps},
    },
    virtmemory::copy_out_cont,
};

static ROOT_FS: Once<Arc<Ext4>> = Once::new();
const ROOT_INODE: u32 = 2;

#[derive(Clone, Copy)]
pub struct FileMetadata {
    pub inode: u64,
    pub mode: u16,
    pub uid: u32,
    pub gid: u32,
    pub links: u32,
    pub size: u64,
    pub blocks: u64,
    pub atime: u32,
    pub mtime: u32,
    pub ctime: u32,
    pub btime: u32,
}

// One open, read-only file on the ext4 root. The offset belongs to the open
// file description, so duplicated descriptors share it as Linux requires.
struct Ext4File {
    fs: Arc<Ext4>,
    inode: u32,
    offset: IntMutex<usize>,
}

// An opened ext4 directory.  The entry index is part of the open file
// description, so duplicated descriptors continue the same iteration.
struct Ext4Directory {
    entries: Vec<DirectoryEntry>,
    position: IntMutex<usize>,
}

struct DirectoryEntry {
    inode: u64,
    file_type: u8,
    name: Vec<u8>,
}

/// Linux's variable-size `linux_dirent64` record. The C ABI ends with a
/// flexible `d_name[]` array, so encode it explicitly instead of relying on
/// Rust's struct layout.
struct LinuxDirent64<'a> {
    inode: u64,
    next_offset: i64,
    file_type: u8,
    name: &'a [u8],
}

impl LinuxDirent64<'_> {
    const HEADER_LEN: usize = 19;

    fn record_len(&self) -> usize {
        (Self::HEADER_LEN + self.name.len() + 1 + 7) & !7
    }

    fn encode(&self) -> Vec<u8> {
        let record_len = self.record_len();
        let mut bytes = alloc::vec![0u8; record_len];
        bytes[0..8].copy_from_slice(&self.inode.to_le_bytes());
        bytes[8..16].copy_from_slice(&self.next_offset.to_le_bytes());
        bytes[16..18].copy_from_slice(&(record_len as u16).to_le_bytes());
        bytes[18] = self.file_type;
        bytes[Self::HEADER_LEN..Self::HEADER_LEN + self.name.len()].copy_from_slice(self.name);
        bytes
    }
}

impl FileOps for Ext4File {
    fn read(&self, proc: &mut Process, addr: usize, len: usize) -> Result<usize, Errno> {
        let mut offset = self.offset.lock();
        let mut data = vec![0; len];
        let count = self
            .fs
            .read_at(self.inode, *offset, &mut data)
            .map_err(|_| Errno::EIO)?;
        copy_out_cont(&mut proc.pagetable, addr, &data[..count]).map_err(|()| Errno::EFAULT)?;
        *offset = offset.checked_add(count).ok_or(Errno::EOVERFLOW)?;
        Ok(count)
    }

    fn write(&self, _proc: &mut Process, _addr: usize, _len: usize) -> Result<usize, Errno> {
        Err(Errno::EROFS)
    }

    fn ioctl(&self, _proc: &mut Process, _op: usize, _arg: usize) -> Result<usize, Errno> {
        Err(Errno::ENOTTY)
    }
}

impl core::fmt::Debug for Ext4Directory {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Ext4Directory")
            .field("position", &*self.position.lock())
            .finish()
    }
}

impl FileOps for Ext4Directory {
    fn read(&self, _proc: &mut Process, _addr: usize, _len: usize) -> Result<usize, Errno> {
        Err(Errno::EISDIR)
    }

    fn write(&self, _proc: &mut Process, _addr: usize, _len: usize) -> Result<usize, Errno> {
        Err(Errno::EROFS)
    }

    fn ioctl(&self, _proc: &mut Process, _op: usize, _arg: usize) -> Result<usize, Errno> {
        Err(Errno::ENOTTY)
    }

    fn getdents64(&self, proc: &mut Process, addr: usize, len: usize) -> Result<usize, Errno> {
        let mut position = self.position.lock();
        let mut written = 0;

        while let Some(entry) = self.entries.get(*position) {
            let record = LinuxDirent64 {
                inode: entry.inode,
                next_offset: *position as i64 + 1,
                file_type: entry.file_type,
                name: &entry.name,
            };
            let record_len = record.record_len();
            if record_len > len - written {
                break;
            }

            let bytes = record.encode();
            copy_out_cont(&mut proc.pagetable, addr + written, &bytes)
                .map_err(|()| Errno::EFAULT)?;
            written += record_len;
            *position += 1;
        }

        if written == 0 && *position < self.entries.len() {
            Err(Errno::EINVAL)
        } else {
            Ok(written)
        }
    }
}

struct VirtioExt4Device {
    disk: SharedBlockDevice,
}

impl VirtioExt4Device {
    #[cfg(feature = "fs-write-probe")]
    fn write_exact(&self, offset: usize, data: &[u8]) -> Result<(), ()> {
        let mut disk = self.disk.lock();
        let size = disk.block_size();
        let end = offset.checked_add(data.len()).ok_or(())?;
        let capacity = disk.block_count().checked_mul(size as u64).ok_or(())?;
        if end as u64 > capacity {
            return Err(());
        }
        let mut block = vec![0; size];
        let mut copied = 0;
        while copied < data.len() {
            let position = offset + copied;
            let index = (position / size) as u64;
            let within = position % size;
            let count = min(size - within, data.len() - copied);
            if within != 0 || count != size {
                disk.read_block(index, &mut block)?;
            }
            block[within..within + count].copy_from_slice(&data[copied..copied + count]);
            disk.write_block(index, &block)?;
            copied += count;
        }
        Ok(())
    }

    fn read_exact(&self, offset: usize) -> Result<Vec<u8>, ()> {
        let mut disk = self.disk.lock();
        let disk_block_size = disk.block_size();
        let mut data = vec![0; BLOCK_SIZE];
        let mut block = vec![0; disk_block_size];
        let mut copied = 0;

        while copied < data.len() {
            let position = offset.checked_add(copied).ok_or(())?;
            let block_idx = (position / disk_block_size) as u64;
            let in_block = position % disk_block_size;
            disk.read_block(block_idx, &mut block)?;

            let count = min(disk_block_size - in_block, data.len() - copied);
            data[copied..copied + count].copy_from_slice(&block[in_block..in_block + count]);
            copied += count;
        }

        Ok(data)
    }
}

impl Ext4BlockDevice for VirtioExt4Device {
    fn read_offset(&self, offset: usize) -> Vec<u8> {
        self.read_exact(offset).expect("ext4 block read failed")
    }

    fn write_offset(&self, _offset: usize, _data: &[u8]) {
        #[cfg(feature = "fs-write-probe")]
        self.write_exact(_offset, _data)
            .expect("ext4 probe block write failed");

        #[cfg(not(feature = "fs-write-probe"))]
        {
            // The initial root filesystem is deliberately read-only.  ext4_rs's
            // device trait cannot report I/O failures, so never risk a silent
            // partial write to the boot volume.
            panic!("attempted write to read-only ext4 root filesystem");
        }
    }
}

/// Discover the first VirtIO-MMIO disk and mount it as the read-only root.
pub fn mount_root() -> Result<(), ()> {
    let disk = block::discover().ok_or(())?;
    #[cfg(feature = "fs-write-probe")]
    let probe_disk = Arc::clone(&disk);
    let device: Arc<dyn Ext4BlockDevice> = Arc::new(VirtioExt4Device { disk });
    let fs = Arc::new(Ext4::open(device));

    // Force a root-directory read now, while boot diagnostics are available.
    let entries = fs.dir_get_entries(ROOT_INODE);
    log::info!("ext4: mounted root with {} entries", entries.len());
    #[cfg(feature = "fs-write-probe")]
    run_write_probe(&fs, &probe_disk)?;
    ROOT_FS.call_once(|| fs);
    Ok(())
}

/// Deliberately test-only: never enable this feature against a valuable image.
#[cfg(feature = "fs-write-probe")]
fn run_write_probe(fs: &Ext4, disk: &SharedBlockDevice) -> Result<(), ()> {
    // Require an explicit image sentinel before issuing any mutation.
    let inode = lookup_inode(fs, "/m004-probe.txt").map_err(|_| ())?;
    if fs.get_inode_ref(inode).inode.size() != (3 * BLOCK_SIZE) as u64 {
        return Err(());
    }
    let payload = b"M004-cross-block-write";
    // The pinned library underflows when total_blocks_needed is less than
    // the remaining file blocks. Exercise the last two blocks for this spike.
    let offset = 2 * BLOCK_SIZE - 8;
    if fs.write_at(inode, offset, payload).map_err(|error| {
        log::error!("M004 overwrite failed: {:?}", error);
    })? != payload.len()
    {
        return Err(());
    }
    disk.lock().flush()?;
    log::info!("M004_OVERWRITE_OK");
    let child = fs
        .create(ROOT_INODE, "m004-created.txt", 0o100644)
        .map_err(|error| {
            log::error!("M004 create failed: {:?}", error);
        })?;
    let data = vec![b'Z'; 2 * BLOCK_SIZE + 37];
    if fs.write_at(child.inode_num, 0, &data).map_err(|error| {
        log::error!("M004 allocation/write failed: {:?}", error);
    })? != data.len()
    {
        return Err(());
    }
    disk.lock().flush()?;
    log::info!("M004_WRITE_PROBE_OK");
    Ok(())
}

/// Resolve an absolute path in the mounted root filesystem.
fn lookup_inode(fs: &Ext4, path: &str) -> Result<u32, Errno> {
    if path.is_empty() || path == "/" {
        return Ok(ROOT_INODE);
    }

    let mut parent = ROOT_INODE;
    fs.generic_open(path, &mut parent, false, 0, &mut 0)
        .map_err(|_| Errno::ENOENT)
}

/// Read a regular file from the mounted root by absolute ext4 path.
pub fn read_file(path: &str) -> Result<Vec<u8>, ()> {
    let fs = ROOT_FS.get().ok_or(())?;
    let inode = lookup_inode(fs, path).map_err(|_| ())?;
    let size = usize::try_from(fs.get_inode_ref(inode).inode.size()).map_err(|_| ())?;
    let mut image = vec![0; size];
    let count = fs.read_at(inode, 0, &mut image).map_err(|_| ())?;
    image.truncate(count);
    Ok(image)
}

/// Look up metadata for an absolute path on the mounted root filesystem.
pub fn metadata(path: &str) -> Result<FileMetadata, Errno> {
    let fs = ROOT_FS.get().ok_or(Errno::ENOENT)?;
    let inode = lookup_inode(fs, path)?;
    let inode_ref = fs.get_inode_ref(inode);
    let inode = inode_ref.inode;

    Ok(FileMetadata {
        inode: inode_ref.inode_num as u64,
        mode: inode.mode,
        uid: inode.uid as u32 | ((inode.osd2.l_i_uid_high as u32) << 16),
        gid: inode.gid as u32 | ((inode.osd2.l_i_gid_high as u32) << 16),
        links: inode.links_count as u32,
        size: inode.size(),
        // ext4 records this count in 512-byte sectors, matching statx.
        blocks: inode.blocks as u64 | ((inode.osd2.l_i_blocks_high as u64) << 32),
        atime: inode.atime,
        mtime: inode.mtime,
        ctime: inode.ctime,
        btime: inode.i_crtime,
    })
}

/// Open an absolute path on the mounted read-only root for userspace.
pub fn open_file(path: &str) -> Result<Arc<dyn FileOps>, Errno> {
    let fs = Arc::clone(ROOT_FS.get().ok_or(Errno::ENOENT)?);
    let inode = lookup_inode(&fs, path)?;
    let inode_ref = fs.get_inode_ref(inode);
    if inode_ref.inode.is_dir() {
        return Ok(Arc::new(Ext4Directory {
            entries: fs
                .dir_get_entries(inode)
                .into_iter()
                .map(|entry| DirectoryEntry {
                    inode: entry.inode as u64,
                    file_type: entry.get_de_type(),
                    name: entry.name[..entry.get_name_len()].to_vec(),
                })
                .collect(),
            position: IntMutex::new(0),
        }));
    }
    if !inode_ref.inode.is_file() {
        return Err(Errno::ENOENT);
    }

    Ok(Arc::new(Ext4File {
        fs,
        inode,
        offset: IntMutex::new(0),
    }))
}

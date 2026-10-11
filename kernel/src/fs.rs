use alloc::{sync::Arc, vec, vec::Vec};
use core::cmp::min;

use ext4_rs::{BlockDevice as Ext4BlockDevice, Ext4, BLOCK_SIZE};
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

/// One open, read-only file on the ext4 root. The offset belongs to the open
/// file description, so duplicated descriptors share it as Linux requires.
struct Ext4File {
    fs: Arc<Ext4>,
    inode: u32,
    offset: IntMutex<usize>,
}

impl core::fmt::Debug for Ext4File {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Ext4File")
            .field("inode", &self.inode)
            .field("offset", &*self.offset.lock())
            .finish()
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

struct VirtioExt4Device {
    disk: SharedBlockDevice,
}

impl VirtioExt4Device {
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
        self.read_exact(offset)
            .expect("ext4 block read failed")
    }

    fn write_offset(&self, _offset: usize, _data: &[u8]) {
        // The initial root filesystem is deliberately read-only.  ext4_rs's
        // device trait cannot report I/O failures, so never risk a silent
        // partial write to the boot volume.
        panic!("attempted write to read-only ext4 root filesystem");
    }
}

/// Discover the first VirtIO-MMIO disk and mount it as the read-only root.
pub fn mount_root() -> Result<(), ()> {
    let disk = block::discover().ok_or(())?;
    let device: Arc<dyn Ext4BlockDevice> = Arc::new(VirtioExt4Device { disk });
    let fs = Arc::new(Ext4::open(device));

    // Force a root-directory read now, while boot diagnostics are available.
    let entries = fs.dir_get_entries(ROOT_INODE);
    log::info!("ext4: mounted root with {} entries", entries.len());
    ROOT_FS.call_once(|| fs);
    Ok(())
}

/// Read a regular file from the mounted root by absolute ext4 path.
pub fn read_file(path: &str) -> Result<Vec<u8>, ()> {
    let fs = ROOT_FS.get().ok_or(())?;
    let mut parent = ROOT_INODE;
    let inode = fs
        .generic_open(path, &mut parent, false, 0, &mut 0)
        .map_err(|_| ())?;

    let mut image = Vec::new();
    let mut offset = 0;
    let mut chunk = vec![0; BLOCK_SIZE];
    loop {
        let count = fs.read_at(inode, offset, &mut chunk).map_err(|_| ())?;
        if count == 0 {
            break;
        }
        image.extend_from_slice(&chunk[..count]);
        offset = offset.checked_add(count).ok_or(())?;
    }
    Ok(image)
}

/// Open an absolute path on the mounted read-only root for userspace.
pub fn open_file(path: &str) -> Result<Arc<dyn FileOps>, Errno> {
    let fs = Arc::clone(ROOT_FS.get().ok_or(Errno::ENOENT)?);
    let inode = if path.is_empty() || path == "/" {
        ROOT_INODE
    } else {
        let mut parent = ROOT_INODE;
        fs.generic_open(path, &mut parent, false, 0, &mut 0)
            .map_err(|_| Errno::ENOENT)?
    };
    let inode_ref = fs.get_inode_ref(inode);
    if inode_ref.inode.is_dir() {
        return Err(Errno::EISDIR);
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

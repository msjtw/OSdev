use alloc::{sync::Arc, vec, vec::Vec};
use core::cmp::min;

use ext4_rs::{BLOCK_SIZE, BlockDevice as Ext4BlockDevice, Ext4};
use spin::Once;

use crate::{
    drivers::block::{self, BlockDevice, SharedBlockDevice},
    log,
};

static ROOT_FS: Once<Arc<Ext4>> = Once::new();
pub(crate) const ROOT_INODE: u32 = 2;

pub(crate) fn root() -> Option<Arc<Ext4>> {
    ROOT_FS.get().map(Arc::clone)
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
        self.read_exact(offset).expect("ext4 block read failed")
    }

    fn write_offset(&self, _offset: usize, _data: &[u8]) {
        panic!("attempted write to read-only ext4 root filesystem");
    }
}

pub fn mount_root() -> Result<(), ()> {
    let disk = block::discover().ok_or(())?;
    let device: Arc<dyn Ext4BlockDevice> = Arc::new(VirtioExt4Device { disk });
    let fs = Arc::new(Ext4::open(device));

    let entries = fs.dir_get_entries(ROOT_INODE);
    log::info!("ext4: mounted root with {} entries", entries.len());

    ROOT_FS.call_once(|| fs);
    Ok(())
}

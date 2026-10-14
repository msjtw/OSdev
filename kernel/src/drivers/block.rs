pub mod virtio_blk;

use alloc::{boxed::Box, sync::Arc};

use crate::{drivers::virtio::transport::Mmio, lock::IntMutex};

pub const VIRTIO_QUEUE_SIZE: usize = 8;
pub type VirtioMmioBlock = virtio_blk::VirtioBlk<Mmio, VIRTIO_QUEUE_SIZE>;
pub type SharedBlockDevice = Arc<IntMutex<Box<VirtioMmioBlock>>>;

pub trait BlockDevice {
    type Error;

    fn block_size(&self) -> usize;
    fn block_count(&self) -> u64;

    fn read_block(&mut self, block_idx: u64, buf: &mut [u8]) -> Result<(), Self::Error>;

    fn write_block(&mut self, block_idx: u64, buf: &[u8]) -> Result<(), Self::Error>;

    fn flush(&mut self) -> Result<(), Self::Error>;
}

pub fn discover() -> Option<SharedBlockDevice> {
    fn probe(base: usize) -> Option<SharedBlockDevice> {
        unsafe fn mmio_read_u32(base: usize, off: usize) -> u32 {
            unsafe { core::ptr::read_volatile((base + off) as *const u32) }
        }

        const TARGET_MAGIC: u32 = 0x74726976;
        const TARGET_DEVICE_ID: u32 = 2;
        const OFF_MAGIC: usize = 0x000;
        const OFF_DEVICE_ID: usize = 0x008;

        use crate::log;

        let magic = unsafe { mmio_read_u32(base, OFF_MAGIC) };
        if magic != TARGET_MAGIC {
            return None;
        }

        let device_id = unsafe { mmio_read_u32(base, OFF_DEVICE_ID) };
        if device_id != TARGET_DEVICE_ID {
            return None;
        }

        log::info!("virtio-blk: found block device at 0x{base:08x}");
        let mut blk = Box::new(VirtioMmioBlock::new(base));
        if blk.init().is_err() {
            log::error!("virtio-blk: initialization failed at 0x{base:08x}");
            return None;
        }
        Some(Arc::new(IntMutex::new(blk)))
    }

    let bases = [
        0x1000_1000,
        0x1000_2000,
        0x1000_3000,
        0x1000_4000,
        0x1000_5000,
        0x1000_6000,
        0x1000_7000,
        0x1000_8000,
    ];

    bases.into_iter().find_map(probe)
}

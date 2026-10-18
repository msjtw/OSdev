pub mod virtio_blk;

use alloc::{boxed::Box, sync::Arc, vec, vec::Vec};
use core::sync::atomic::{AtomicUsize, Ordering};
use spin::Once;

use crate::{drivers::virtio::transport::Mmio, lock::IntMutex};

pub const VIRTIO_QUEUE_SIZE: usize = 8;
pub type VirtioMmioBlock = virtio_blk::VirtioBlk<Mmio, VIRTIO_QUEUE_SIZE>;
pub const CACHE_BLOCKS: usize = 16;
pub type CachedVirtioMmioBlock = CachedBlockDevice<VirtioMmioBlock>;
pub type SharedBlockDevice = Arc<IntMutex<Box<CachedVirtioMmioBlock>>>;

static DISCOVERED: Once<SharedBlockDevice> = Once::new();
static TIMER_TICKS: AtomicUsize = AtomicUsize::new(0);
const PERIODIC_FLUSH_TICKS: usize = 10;

pub trait BlockDevice {
    type Error;

    fn block_size(&self) -> usize;
    fn block_count(&self) -> u64;

    fn read_block(&mut self, block_idx: u64, buf: &mut [u8]) -> Result<(), Self::Error>;

    fn write_block(&mut self, block_idx: u64, buf: &[u8]) -> Result<(), Self::Error>;

    fn flush(&mut self) -> Result<(), Self::Error>;
}

struct CacheEntry {
    block_idx: u64,
    data: Vec<u8>,
    valid: bool,
    dirty: bool,
    last_used: u64,
}


pub struct CachedBlockDevice<D: BlockDevice<Error = ()>> {
    device: Box<D>,
    entries: Vec<CacheEntry>,
    block_size: usize,
    use_counter: u64,
    last_flush_failed: bool,
}

impl<D: BlockDevice<Error = ()>> CachedBlockDevice<D> {
    pub fn new(device: Box<D>, entries: usize) -> Result<Self, ()> {
        let block_size = device.block_size();
        if block_size == 0 || entries == 0 {
            return Err(());
        }
        Ok(Self {
            device,
            entries: (0..entries)
                .map(|_| CacheEntry {
                    block_idx: 0,
                    data: vec![0; block_size],
                    valid: false,
                    dirty: false,
                    last_used: 0,
                })
                .collect(),
            block_size,
            use_counter: 0,
            last_flush_failed: false,
        })
    }

    pub fn has_dirty(&self) -> bool {
        self.entries.iter().any(|entry| entry.dirty)
    }

    pub fn last_flush_failed(&self) -> bool {
        self.last_flush_failed
    }

    fn touch(&mut self, index: usize) {
        self.use_counter = self.use_counter.wrapping_add(1);
        self.entries[index].last_used = self.use_counter;
    }

    fn victim(&self) -> usize {
        self.entries
            .iter()
            .position(|entry| !entry.valid)
            .unwrap_or_else(|| {
                self.entries
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, entry)| entry.last_used)
                    .map(|(index, _)| index)
                    .expect("block cache has no entries")
            })
    }

    fn flush_entry(&mut self, index: usize) -> Result<(), ()> {
        if !self.entries[index].dirty {
            return Ok(());
        }
        let (device, entries) = (&mut self.device, &mut self.entries);
        let entry = &mut entries[index];
        device.write_block(entry.block_idx, &entry.data)?;
        entry.dirty = false;
        Ok(())
    }

    fn prepare_victim(&mut self) -> Result<usize, ()> {
        let index = self.victim();
        if self.entries[index].dirty {
            if self.flush_entry(index).is_err() {
                self.last_flush_failed = true;
                return Err(());
            }
        }
        Ok(index)
    }
}

impl<D: BlockDevice<Error = ()>> BlockDevice for CachedBlockDevice<D> {
    type Error = ();

    fn block_size(&self) -> usize {
        self.block_size
    }

    fn block_count(&self) -> u64 {
        self.device.block_count()
    }

    fn read_block(&mut self, block_idx: u64, buf: &mut [u8]) -> Result<(), ()> {
        if buf.len() != self.block_size {
            return Err(());
        }
        if let Some(index) = self.entries.iter().position(|entry| entry.valid && entry.block_idx == block_idx) {
            self.touch(index);
            buf.copy_from_slice(&self.entries[index].data);
            return Ok(());
        }

        let index = self.prepare_victim()?;
        self.device.read_block(block_idx, &mut self.entries[index].data)?;
        self.entries[index].block_idx = block_idx;
        self.entries[index].valid = true;
        self.entries[index].dirty = false;
        self.touch(index);
        buf.copy_from_slice(&self.entries[index].data);
        Ok(())
    }

    fn write_block(&mut self, block_idx: u64, buf: &[u8]) -> Result<(), ()> {
        if buf.len() != self.block_size || block_idx >= self.device.block_count() {
            return Err(());
        }
        let index = match self.entries.iter().position(|entry| entry.valid && entry.block_idx == block_idx) {
            Some(index) => index,
            None => self.prepare_victim()?,
        };
        self.entries[index].block_idx = block_idx;
        self.entries[index].data.copy_from_slice(buf);
        self.entries[index].valid = true;
        self.entries[index].dirty = true;
        self.touch(index);
        Ok(())
    }

    fn flush(&mut self) -> Result<(), ()> {
        for index in 0..self.entries.len() {
            if self.flush_entry(index).is_err() {
                self.last_flush_failed = true;
                return Err(());
            }
        }
        if self.device.flush().is_err() {
            self.last_flush_failed = true;
            return Err(());
        }
        self.last_flush_failed = false;
        Ok(())
    }
}

pub fn discover() -> Option<SharedBlockDevice> {
    if let Some(device) = DISCOVERED.get() {
        return Some(Arc::clone(device));
    }
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
        let cache = CachedBlockDevice::new(blk, CACHE_BLOCKS).ok()?;
        Some(Arc::new(IntMutex::new(Box::new(cache))))
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

    let device = bases.into_iter().find_map(probe)?;
    DISCOVERED.call_once(|| Arc::clone(&device));
    Some(device)
}

pub fn flush_all() -> Result<(), ()> {
    let Some(device) = DISCOVERED.get() else {
        return Ok(());
    };
    device.lock().flush()
}

pub fn periodic_flush() {
    if TIMER_TICKS.fetch_add(1, Ordering::Relaxed) % PERIODIC_FLUSH_TICKS != 0 {
        return;
    }
    let Some(device) = DISCOVERED.get() else {
        return;
    };
    let Some(mut device) = device.try_lock() else {
        return;
    };
    if device.has_dirty() && device.flush().is_err() {
        crate::log::error!("block cache: periodic flush failed; dirty blocks retained");
    }
}

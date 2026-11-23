// https://docs.oasis-open.org/virtio/virtio/v1.0/cs01/virtio-v1.0-cs01.html
// 4.2: Virtio over MMIO
// 5.2: Block Device

const SECTOR_SIZE: usize = 512;

const KIND_IN: u32 = 0;
const KIND_OUT: u32 = 1;
const KIND_FLUSH: u32 = 4;
const KIND_GET_ID: u32 = 8;
const KIND_GET_LIFETIME: u32 = 10;
const KIND_DISCARD: u32 = 11;
const KIND_WRITE_ZEROES: u32 = 13;
const KIND_SECURE_ERASE: u32 = 14;

const STATUS_OK: u8 = 0;
const STATUS_IOERR: u8 = 1;
const STATUS_UNSUPP: u8 = 2;

const FEAT_BLOCK_SIZE: u64 = 1 << 6;
const FEAT_FLUSH: u64 = 1 << 9;
const FEAT_VIRTIO_VERSION_1: u64 = 1 << 32;

const CONFIG_CAPACITY: usize = 0x00;
const CONFIG_BLOCK_SIZE: usize = 0x14;

use crate::log;

use crate::dma::{DEFAULT_BOUNCE_CAPACITY, Layout, Region};
use crate::virtio::SplitQueue;
use crate::virtio::Transport;
use crate::virtio::status;
use crate::virtio::transport::Mmio;

#[repr(C)]
pub struct ReqHeader {
    kind: u32,
    reserved: u32,
    sector: u64,
}

pub struct VirtioBlk<T: Transport, const N: usize> {
    pending: Option<Operation>,
    read_ready: bool,
    failed: bool,
    transport: T,
    region: Region,
    queue: &'static mut SplitQueue<N>,
    capacity: u64,
    block_size: usize,
    // Device-visible data lives at stable workspace addresses, never in the
    // driver object or request locals. Descriptors use translated addresses.
    request_header: &'static mut ReqHeader,
    request_status: &'static mut u8,
    bounce: &'static mut [u8],
}

impl<const N: usize> VirtioBlk<Mmio, N> {
    /// # Safety
    /// Region must be exclusively owned, writable, mapped at virtual_base and
    /// backed by physical_base for its entire lifetime, including outstanding DMA.
    /// It must not be unmapped or reclaimed even if this driver is dropped.
    pub unsafe fn from_workspace(base_addr: usize, region: Region) -> Result<Self, ()> {
        if N != 8 {
            return Err(());
        }
        let layout = Layout::new(DEFAULT_BOUNCE_CAPACITY).ok_or(())?;
        let region = Region::new(
            region.virtual_base,
            region.physical_base,
            region.size,
            layout,
        )
        .ok_or(())?;
        let queue = (region.virtual_base + layout.queue) as *mut SplitQueue<N>;
        let header = (region.virtual_base + layout.header) as *mut ReqHeader;
        let status = (region.virtual_base + layout.status) as *mut u8;
        unsafe {
            queue.write(SplitQueue::new());
            header.write(ReqHeader {
                kind: 0,
                reserved: 0,
                sector: 0,
            });
            status.write(0xff);
        }
        Ok(Self {
            pending: None,
            read_ready: false,
            failed: false,
            transport: Mmio { base_addr },
            region,
            queue: unsafe { &mut *queue },
            capacity: 0,
            block_size: 0,
            request_header: unsafe { &mut *header },
            request_status: unsafe { &mut *status },
            bounce: unsafe {
                core::slice::from_raw_parts_mut(
                    (region.virtual_base + layout.bounce) as *mut u8,
                    layout.bounce_capacity,
                )
            },
        })
    }

    pub fn init(&mut self) -> Result<(), ()> {
        log::debug!("virtio-blk: initializing with mmio transport");

        self.check_mmio_magic()?;
        self.check_mmio_version()?;
        self.check_vendor_id()?;
        self.check_device_id()?;

        self.acknowledge_driver()?;

        let required_features = FEAT_VIRTIO_VERSION_1 | FEAT_FLUSH;

        let supported_features = required_features | FEAT_BLOCK_SIZE;

        self.negotiate_features(required_features, supported_features)?;

        // VIRTIO_BLK_F_BLK_SIZE is optional; without it the specification
        // defines a 512-byte logical block size and the config field is not
        // valid (QEMU returns zero there).
        self.block_size = if self.transport.device_features() & FEAT_BLOCK_SIZE != 0 {
            self.transport.read_config_u32(CONFIG_BLOCK_SIZE) as usize
        } else {
            SECTOR_SIZE
        };
        if self.block_size == 0 {
            // Some QEMU versions advertise BLK_SIZE but leave the optional
            // configuration value at zero.  The virtio default remains 512.
            self.block_size = SECTOR_SIZE;
        }
        if self.block_size < 512 || !self.block_size.is_power_of_two() {
            log::error!("virtio-blk: invliad block size: {}", self.block_size);
            return Err(());
        }
        // Use a permanent DMA-safe buffer. Callers can supply stack or
        // virtual-memory buffers, neither of which may be physical addresses.
        if self.block_size > self.bounce.len() {
            return Err(());
        }
        let capacity_sectors = self.transport.read_config_u64(CONFIG_CAPACITY);
        self.capacity = capacity_sectors / (self.block_size / SECTOR_SIZE) as u64;
        if self.capacity == 0 {
            log::error!("virtio-blk: device has no addressable blocks");
            return Err(());
        }
        log::debug!("virtio-blk: block size: {}", self.block_size);

        self.basic_queue_setup(0)?;

        log::info!("virtio-blk: initialization with mmio transport successful");

        Ok(())
    }

    fn check_mmio_magic(&self) -> Result<(), ()> {
        let expected = 0x7472_6976;
        match self.transport.magic() {
            x if x == expected => {
                log::trace!("virtio-blk: read mmio magic: 0x{x:08x}");
                Ok(())
            }
            x => {
                log::error!("virtio-blk: invalid mmio magic: 0x{x:08x}");
                Err(())
            }
        }
    }

    fn check_mmio_version(&self) -> Result<(), ()> {
        let expected = 0x0000_0002;
        match self.transport.version() {
            x if x == expected => {
                log::trace!("virtio-blk: read mmio version: 0x{x:08x}");
                Ok(())
            }
            x => {
                log::error!("virtio-blk: invalid mmio version: 0x{x:08x}");
                Err(())
            }
        }
    }
}

impl<T: Transport, const N: usize> VirtioBlk<T, N> {
    fn check_vendor_id(&self) -> Result<(), ()> {
        let expected = 0x554d_4551;
        match self.transport.vendor_id() {
            x if x == expected => {
                log::trace!("virtio-blk: read vendor id: 0x{x:08x}");
                Ok(())
            }
            x => {
                log::error!("virtio-blk: invalid vendor id: 0x{x:08x}");
                Err(())
            }
        }
    }
    fn check_device_id(&self) -> Result<(), ()> {
        //let expected = 0x554d4551;
        let expected = 0x0000_0002;
        match self.transport.device_id() {
            x if x == expected => {
                log::trace!("virtio-blk: read device id: 0x{x:08x}");
                Ok(())
            }
            x => {
                log::error!("virtio-blk: invalid device id: 0x{x:08x}");
                Err(())
            }
        }
    }

    fn acknowledge_driver(&mut self) -> Result<(), ()> {
        self.transport.set_status_checked(0).map_err(|_| {
            log::error!("virtio-blk: failed to reset device");
        })?;

        self.transport
            .set_status_checked(status::ACKNOWLEDGE)
            .map_err(|_| {
                log::error!("virtio-blk: failed to set status ACKNOWLEDGE");
            })?;

        self.transport
            .set_status_checked(status::ACKNOWLEDGE | status::DRIVER)
            .map_err(|_| {
                log::error!("virtio-blk: failed to set status DRIVER");
            })?;

        Ok(())
    }

    fn negotiate_features(&mut self, required_feats: u64, supported_feats: u64) -> Result<(), ()> {
        log::trace!("virtio-blk: required features:  0x{required_feats:016x}");
        log::trace!("virtio-blk: supported features: 0x{supported_feats:016x}");

        let device_feats = self.transport.device_features();
        log::trace!("virtio-blk: device features:    0x{device_feats:016x}");

        if required_feats != required_feats & device_feats {
            log::error!("virtio-blk: device missing required features");
            return Err(());
        }

        let common_feats = supported_feats & device_feats;
        log::trace!("virtio-blk: common features:    0x{common_feats:016x}");

        self.transport.set_driver_features(common_feats);
        // TODO (maybe) make it checked

        self.transport
            .set_status_checked(status::ACKNOWLEDGE | status::DRIVER | status::FEATURES_OK)
            .map_err(|_| {
                log::error!("virtio-blk: failed to set status FEATURES_OK");
            })?;

        Ok(())
    }

    fn basic_queue_setup(&mut self, queue_idx: u16) -> Result<(), ()> {
        self.transport.select_queue(queue_idx);
        log::trace!("virtio-blk: setting size {N} for queue {queue_idx}");

        let queue_max_size = self.transport.queue_max_size();
        if N > queue_max_size as usize {
            log::error!("virtio-blk: device queue max size is {queue_max_size}");
            return Err(());
        }

        self.transport.set_queue_size(N as u16);

        self.transport.set_queue_addresses(
            self.region
                .device_address(
                    self.queue.desc_area_addr() as usize,
                    core::mem::size_of_val(&self.queue.desc_area),
                )
                .ok_or(())?,
            self.region
                .device_address(
                    self.queue.driver_area_addr() as usize,
                    core::mem::size_of_val(&self.queue.driver_area),
                )
                .ok_or(())?,
            self.region
                .device_address(
                    self.queue.device_area_addr() as usize,
                    core::mem::size_of_val(&self.queue.device_area),
                )
                .ok_or(())?,
        );

        self.transport.enable_queue();
        self.transport.set_status_bits(status::DRIVER_OK);

        Ok(())
    }

    /// Check once; invalid completion IDs permanently prevent DMA storage reuse.
    pub fn poll_complete(&mut self) -> Result<Option<Operation>, ()> {
        if self.failed {
            return Err(());
        }
        let count = unsafe { core::ptr::read_volatile(&self.queue.device_area.idx) }
            .wrapping_sub(self.queue.last_dev_area_idx);
        if count > 1 {
            self.failed = true;
            return Err(());
        }
        let Some(elem) = self.queue.pop_optional() else {
            return Ok(None);
        };
        if elem.id != 0 || self.pending.is_none() {
            self.failed = true;
            return Err(());
        }
        let operation = self.pending.take().unwrap();
        let status = unsafe { core::ptr::read_volatile(self.request_status) };
        self.validate_request_status(status)?;
        self.read_ready = operation == Operation::Read;
        Ok(Some(operation))
    }

    fn available(&self) -> Result<(), ()> {
        if self.failed || self.pending.is_some() {
            Err(())
        } else {
            Ok(())
        }
    }

    /// Read data is borrowed until the next mutable driver operation.
    pub fn read_data(&self) -> Result<&[u8], ()> {
        self.available()?;
        if !self.read_ready {
            return Err(());
        }
        let status = unsafe { core::ptr::read_volatile(self.request_status) };
        self.validate_request_status(status)?;
        Ok(&self.bounce[..self.block_size])
    }

    #[inline]
    fn validate_request_status(&self, status: u8) -> Result<(), ()> {
        match status {
            STATUS_OK => Ok(()),
            STATUS_IOERR => {
                log::error!("virtio-blk: request failed with io error");
                Err(())
            }
            STATUS_UNSUPP => {
                log::error!("virtio-blk: request unsupported by device");
                Err(())
            }
            other => {
                log::error!("virtio-blk: request failed unknown error: {other}");
                Err(())
            }
        }
    }

    #[inline]
    fn validate_buffer(&self, buf: &[u8]) -> Result<(), ()> {
        if buf.len() != self.block_size {
            log::error!(
                "virtio-blk: read buffer has size {}, expected {}",
                buf.len(),
                self.block_size,
            );
            Err(())
        } else {
            Ok(())
        }
    }

    #[inline]
    fn sector_from_block_idx(&self, block_idx: u64) -> Result<u64, ()> {
        if block_idx >= self.capacity {
            log::error!(
                "virtio-blk: cannot access block {}, capacity is {}",
                block_idx,
                self.capacity,
            );
            Err(())
        } else {
            let sectors_per_block = self.block_size / SECTOR_SIZE;
            block_idx.checked_mul(sectors_per_block as u64).ok_or(())
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Read,
    Write,
    Flush,
}

impl<T: Transport, const N: usize> VirtioBlk<T, N> {
    pub fn block_size(&self) -> usize {
        self.block_size
    }
    pub fn block_count(&self) -> u64 {
        self.capacity
    }

    pub fn submit_read(&mut self, block_idx: u64) -> Result<(), ()> {
        self.available()?;

        let sector_idx = self.sector_from_block_idx(block_idx)?;

        *self.request_header = ReqHeader {
            kind: KIND_IN,
            reserved: 0,
            sector: sector_idx,
        };
        *self.request_status = 0xff;

        let header_addr = self
            .region
            .device_address(
                self.request_header as *const ReqHeader as usize,
                core::mem::size_of::<ReqHeader>(),
            )
            .ok_or(())? as usize;
        let data_addr = self
            .region
            .device_address(self.bounce.as_ptr() as usize, self.block_size)
            .ok_or(())? as usize;
        let status_addr = self
            .region
            .device_address(self.request_status as *const u8 as usize, 1)
            .ok_or(())? as usize;

        self.queue
            .desc(0)
            .addr_raw(header_addr)
            .len_from_type::<ReqHeader>()
            .next(1);

        self.queue
            .desc(1)
            .addr_raw(data_addr)
            .len(self.block_size)
            .flag_write()
            .next(2);

        self.queue.desc(2).addr_raw(status_addr).len(1).flag_write();

        self.pending = Some(Operation::Read);
        self.read_ready = false;
        self.queue.push(0);
        self.transport.notify_queue(0);

        Ok(())
    }

    pub fn submit_write(&mut self, block_idx: u64, buf: &[u8]) -> Result<(), ()> {
        self.available()?;
        self.validate_buffer(&buf)?;

        let sector_idx = self.sector_from_block_idx(block_idx)?;

        *self.request_header = ReqHeader {
            kind: KIND_OUT,
            reserved: 0,
            sector: sector_idx,
        };
        *self.request_status = 0xff;
        self.bounce[..self.block_size].copy_from_slice(buf);

        let header_addr = self
            .region
            .device_address(
                self.request_header as *const ReqHeader as usize,
                core::mem::size_of::<ReqHeader>(),
            )
            .ok_or(())? as usize;
        let data_addr = self
            .region
            .device_address(self.bounce.as_ptr() as usize, self.block_size)
            .ok_or(())? as usize;
        let status_addr = self
            .region
            .device_address(self.request_status as *const u8 as usize, 1)
            .ok_or(())? as usize;

        self.queue
            .desc(0)
            .addr_raw(header_addr)
            .len_from_type::<ReqHeader>()
            .next(1);

        self.queue
            .desc(1)
            .addr_raw(data_addr)
            .len(self.block_size)
            .next(2);

        self.queue.desc(2).addr_raw(status_addr).len(1).flag_write();

        self.pending = Some(Operation::Write);
        self.read_ready = false;
        self.queue.push(0);
        self.transport.notify_queue(0);

        Ok(())
    }

    pub fn submit_flush(&mut self) -> Result<(), ()> {
        self.available()?;
        *self.request_header = ReqHeader {
            kind: KIND_FLUSH,
            reserved: 0,
            sector: 0,
        };
        *self.request_status = 0xff;

        let header_addr = self
            .region
            .device_address(
                self.request_header as *const ReqHeader as usize,
                core::mem::size_of::<ReqHeader>(),
            )
            .ok_or(())? as usize;
        let status_addr = self
            .region
            .device_address(self.request_status as *const u8 as usize, 1)
            .ok_or(())? as usize;

        self.queue
            .desc(0)
            .addr_raw(header_addr)
            .len_from_type::<ReqHeader>()
            .next(1);

        self.queue.desc(1).addr_raw(status_addr).len(1).flag_write();

        self.pending = Some(Operation::Flush);
        self.read_ready = false;
        self.queue.push(0);
        self.transport.notify_queue(0);

        Ok(())
    }
}

#[cfg(test)]
mod async_tests {
    use super::*;
    #[test]
    fn translated_addresses_sector_and_read_completion() {
        let mut registers = [0u32; 64];
        registers[0x34 / 4] = 8;
        let mut driver = make(registers.as_mut_ptr() as usize);
        driver.block_size = 1024;
        driver.capacity = (1u64 << 32) + 5;
        driver.basic_queue_setup(0).unwrap();
        let layout = Layout::new(DEFAULT_BOUNCE_CAPACITY).unwrap();
        assert_eq!(registers[0x80 / 4], 0x100000);
        assert_eq!(
            registers[0x90 / 4] as u64,
            driver
                .region
                .device_address(driver.queue.driver_area_addr() as usize, 1)
                .unwrap()
        );
        assert_eq!(
            registers[0xa0 / 4] as u64,
            driver
                .region
                .device_address(driver.queue.device_area_addr() as usize, 1)
                .unwrap()
        );
        assert!(driver.submit_read(driver.capacity).is_err());
        driver.submit_read((1u64 << 32) + 1).unwrap();
        assert_eq!(driver.request_header.sector, ((1u64 << 32) + 1) * 2);
        assert_eq!(
            driver.queue.desc_area.entries[0].addr,
            0x100000 + layout.header as u64
        );
        assert_eq!(driver.queue.desc_area.entries[0].flags, 1);
        assert_eq!(
            driver.queue.desc_area.entries[1].addr,
            0x100000 + layout.bounce as u64
        );
        assert_eq!(driver.queue.desc_area.entries[1].len, 1024);
        assert_eq!(driver.queue.desc_area.entries[1].flags, 3);
        assert_eq!(
            driver.queue.desc_area.entries[2].addr,
            0x100000 + layout.status as u64
        );
        assert_eq!(driver.queue.desc_area.entries[2].flags, 2);
        driver.bounce[..1024].fill(9);
        *driver.request_status = STATUS_OK;
        driver.queue.device_area.idx = 1;
        assert_eq!(driver.poll_complete(), Ok(Some(Operation::Read)));
        assert_eq!(driver.read_data().unwrap(), &[9; 1024]);
        assert_eq!(driver.poll_complete(), Ok(None));
        driver.submit_flush().unwrap();
        assert_eq!(driver.queue.desc_area.entries[0].next, 1);
        assert_eq!(driver.queue.desc_area.entries[1].flags, 2);
        assert_eq!(driver.queue.desc_area.entries[1].len, 1);
    }

    #[test]
    fn device_errors_release_completed_work_but_ring_corruption_does_not() {
        let mut registers = [0u32; 64];
        let mut driver = make(registers.as_mut_ptr() as usize);
        for (i, status) in [STATUS_IOERR, STATUS_UNSUPP, 0xff].into_iter().enumerate() {
            driver.submit_flush().unwrap();
            *driver.request_status = status;
            driver.queue.device_area.ring[i].id = 0;
            driver.queue.device_area.idx = (i + 1) as u16;
            assert_eq!(driver.poll_complete(), Err(()));
            assert!(driver.pending.is_none());
            assert!(driver.read_data().is_err());
        }
        driver.submit_flush().unwrap();
        driver.queue.device_area.idx = driver.queue.last_dev_area_idx.wrapping_add(2);
        assert_eq!(driver.poll_complete(), Err(()));
        assert!(driver.submit_flush().is_err());
        assert_eq!(driver.pending, Some(Operation::Flush));
    }
    fn make(base: usize) -> VirtioBlk<Mmio, 8> {
        let layout = Layout::new(DEFAULT_BOUNCE_CAPACITY).unwrap();
        let ptr = unsafe {
            alloc::alloc::alloc_zeroed(
                core::alloc::Layout::from_size_align(layout.size, 4096).unwrap(),
            )
        };
        assert!(!ptr.is_null());
        let region = Region::new(ptr as usize, 0x100000, layout.size, layout).unwrap();
        unsafe { VirtioBlk::from_workspace(base, region).unwrap() }
    }
    #[test]
    fn pending_requests_do_not_wait_or_allow_buffer_reuse() {
        let mut registers = [0u32; 64];
        let mut driver = make(registers.as_mut_ptr() as usize);
        driver.capacity = 10;
        driver.block_size = 512;
        driver.submit_write(1, &[7; 512]).unwrap();
        assert_eq!(driver.poll_complete(), Ok(None));
        assert!(driver.submit_read(0).is_err());
        assert!(driver.submit_flush().is_err());
        assert_eq!(driver.bounce[..512], [7; 512]);
        assert_eq!(driver.queue.driver_area.idx, 1);
        *driver.request_status = STATUS_OK;
        driver.queue.device_area.ring[0].id = 0;
        driver.queue.device_area.idx = 1;
        assert_eq!(driver.poll_complete(), Ok(Some(Operation::Write)));
        assert!(driver.read_data().is_err());
        assert_eq!(driver.poll_complete(), Ok(None));
    }
    #[test]
    fn invalid_completion_fails_closed() {
        let mut registers = [0u32; 64];
        let mut driver = make(registers.as_mut_ptr() as usize);
        driver.submit_flush().unwrap();
        driver.queue.device_area.ring[0].id = 3;
        driver.queue.device_area.idx = 1;
        assert_eq!(driver.poll_complete(), Err(()));
        assert!(driver.submit_flush().is_err());
        assert_eq!(driver.pending, Some(Operation::Flush));
    }
}

/// Legacy manual diagnostic, not part of server progress.
/// # Safety
/// The workspace must satisfy from_workspace's mapping and lifetime contract.
pub unsafe fn test(base_addr: usize, region: Region) {
    const QUEUE_SIZE: usize = 8;

    let Ok(blk) = (unsafe { VirtioBlk::<Mmio, QUEUE_SIZE>::from_workspace(base_addr, region) })
    else {
        return;
    };
    let mut blk = alloc::boxed::Box::pin(blk);

    if let Err(_) = blk.init() {
        log::error!("virtio-blk: killing myself, bye vro...");
        return;
    };

    /********************/
    /*** QUEUE CONFIG ***/
    /********************/

    /************************/
    /*** SEND THE REQUEST ***/
    /************************/

    let header = ReqHeader {
        kind: KIND_IN,
        reserved: 0,
        sector: 0,
    };

    let mut buf = [0_u8; 512];

    if blk.submit_read(0).is_err() {
        return;
    }
    loop {
        match blk.poll_complete() {
            Ok(None) => core::hint::spin_loop(),
            Ok(Some(_)) => break,
            Err(()) => return,
        }
    }
    let Ok(data) = blk.read_data() else {
        return;
    };
    if data.len() != buf.len() {
        return;
    }
    buf.copy_from_slice(data);

    // read string from block 0
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    let Ok(msg) = core::str::from_utf8(&buf[..end]) else {
        log::uart_println!("not a valid string: {:?}", &buf[..end]);
        return;
    };

    log::uart_println!("=== DISK MESSAGE BELOW ===");
    log::uart_println!("{msg}");

    /*
    let status = 0xff_u8;
    let block_size = blk.block_size();
    blk.queue
        .desc(0)
        .addr(&header)
        .len_from_type::<ReqHeader>()
        .next(1);
    blk.queue
        .desc(1)
        .addr(&buf)
        .len(block_size)
        .flag_write()
        .next(2);
    blk.queue
        .desc(2)
        .addr(&status)
        .len(1)
        .flag_write();

    log::uart_println!(
        "avail idx before push = {}",
        blk.queue.driver_area.idx
    );

    blk.queue.push(0);

    log::uart_println!(
        "avail idx after push = {}",
        blk.queue.driver_area.idx
    );

    blk.transport.notify_queue(0);
    log::uart_println!("queue notified");

    let mut counter = 0_usize;
    let out = loop {
        counter += 1;
        if let Some(elem) = blk.queue.pop_optional() {
            break elem;
        }

        core::hint::spin_loop();
        if counter % 0x40000 == 0 {
            log::uart_println!("waiting...");
            log::uart_println!(
                "avail.idx={} used.idx={} status={:#x}",
                unsafe {
                    core::ptr::read_volatile(
                        &blk.queue.driver_area.idx
                    )
                },
                unsafe {
                    core::ptr::read_volatile(
                        &blk.queue.device_area.idx
                    )
                },
                status,
            );
        }
    };

    if out.id != 0 {
        log::uart_println!("id is {}, should be 0", out.id);
        return;
    }

    log::uart_println!("status: 0x{status:02x}");
    // read string from block 0
    let end = buf
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(buf.len());
    let Ok(msg) = core::str::from_utf8(&buf[..end]) else {
        log::uart_println!("not a valid string: {:?}", &buf[..end]);
        return;
    };

    log::uart_println!("=== DISK MESSAGE BELOW ===");
    log::uart_println!("{msg}");
    */
}

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

use crate::drivers::block::BlockDevice;
use crate::drivers::virtio::SplitQueue;
use crate::drivers::virtio::Transport;
use crate::drivers::virtio::queue::Desc;
use crate::drivers::virtio::status;
use crate::drivers::virtio::transport::Mmio;

use core::mem::size_of;

#[repr(C)]
pub struct ReqHeader {
    kind: u32,
    reserved: u32,
    sector: u64,
}

pub struct VirtioBlk<T: Transport, const N: usize> {
    transport: T,
    queue: SplitQueue<N>,
    capacity: u64,
    block_size: usize,
}

impl<const N: usize> VirtioBlk<Mmio, N> {
    pub fn new(base_addr: usize) -> Self {
        Self {
            transport: Mmio { base_addr },
            queue: SplitQueue::<N>::new(),
            capacity: 0,
            block_size: 0,
        }
    }

    pub fn init(&mut self) -> Result<(), ()> {
        log::debug!("virtio-blk: initializing with mmio transport");

        self.check_mmio_magic()?;
        self.check_mmio_version()?;
        self.check_vendor_id()?;
        self.check_device_id()?;

        self.capacity = self.transport.read_config_u64(CONFIG_CAPACITY);

        self.acknowledge_driver()?;

        let required_features = FEAT_VIRTIO_VERSION_1 | FEAT_FLUSH;

        let supported_features = required_features | FEAT_BLOCK_SIZE;

        self.negotiate_features(required_features, supported_features)?;

        self.block_size = self.transport.read_config_u32(CONFIG_BLOCK_SIZE) as usize;
        if self.block_size < 512 || !self.block_size.is_power_of_two() {
            log::error!("virtio-blk: invliad block size: {}", self.block_size);
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
            self.queue.desc_area_addr(),
            self.queue.driver_area_addr(),
            self.queue.device_area_addr(),
        );

        self.transport.enable_queue();
        self.transport.set_status_bits(status::DRIVER_OK);

        Ok(())
    }

    fn wait_for_request(&mut self, expected_head: u32) -> Result<(), ()> {
        loop {
            if let Some(elem) = self.queue.pop_optional() {
                if elem.id != expected_head {
                    log::error!(
                        "virtio-blk: unexpected used descriptor id {}, expected {}",
                        elem.id,
                        expected_head,
                    );
                    return Err(());
                }
                return Ok(());
            }

            core::hint::spin_loop();
        }
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
        if block_idx > self.capacity {
            log::error!(
                "virtio-blk: cannot access block {}, capacity is {}",
                block_idx,
                self.capacity,
            );
            Err(())
        } else {
            let sectors_per_block = self.block_size / SECTOR_SIZE;
            Ok((block_idx * (sectors_per_block as u64)))
        }
    }
}

impl<T: Transport, const N: usize> BlockDevice for VirtioBlk<T, N> {
    type Error = ();

    fn block_size(&self) -> usize {
        self.block_size
    }
    fn block_count(&self) -> u64 {
        self.capacity
    }

    fn read_block(&mut self, block_idx: u64, buf: &mut [u8]) -> Result<(), ()> {
        self.validate_buffer(&buf)?;

        let sector_idx = self.sector_from_block_idx(block_idx)?;

        let header = ReqHeader {
            kind: KIND_IN,
            reserved: 0,
            sector: sector_idx,
        };

        let mut status = 0xff_u8;

        self.queue
            .desc(0)
            .addr(&header)
            .len_from_type::<ReqHeader>()
            .next(1);

        self.queue
            .desc(1)
            .addr(buf)
            .len(self.block_size)
            .flag_write()
            .next(2);

        self.queue.desc(2).addr(&status).len(1).flag_write();

        self.queue.push(0);
        self.transport.notify_queue(0);

        self.wait_for_request(0)?;
        self.validate_request_status(status)?;

        Ok(())
    }

    fn write_block(&mut self, block_idx: u64, buf: &[u8]) -> Result<(), ()> {
        self.validate_buffer(&buf)?;

        let sector_idx = self.sector_from_block_idx(block_idx)?;

        let header = ReqHeader {
            kind: KIND_OUT,
            reserved: 0,
            sector: sector_idx,
        };

        let mut status = 0xff_u8;

        self.queue
            .desc(0)
            .addr(&header)
            .len_from_type::<ReqHeader>()
            .next(1);

        self.queue
            .desc(1)
            .addr(buf)
            .len(self.block_size)
            .flag_write()
            .next(2);

        self.queue.desc(2).addr(&status).len(1).flag_write();

        self.queue.push(0);
        self.transport.notify_queue(0);

        self.wait_for_request(0)?;
        self.validate_request_status(status)?;

        Ok(())
    }

    fn flush(&mut self) -> Result<(), ()> {
        let header = ReqHeader {
            kind: KIND_FLUSH,
            reserved: 0,
            sector: 0,
        };

        let mut status = 0xff_u8;

        self.queue
            .desc(0)
            .addr(&header)
            .len_from_type::<ReqHeader>()
            .next(1);

        self.queue.desc(1).addr(&status).len(1).flag_write();

        self.queue.push(0);
        self.transport.notify_queue(0);

        self.wait_for_request(0)?;
        self.validate_request_status(status)?;

        Ok(())
    }
}

pub fn test(base_addr: usize) {
    const QUEUE_SIZE: usize = 8;

    let blk = VirtioBlk::<Mmio, QUEUE_SIZE>::new(base_addr);
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

    if let Err(_) = blk.read_block(0, &mut buf) {
        return;
    }

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

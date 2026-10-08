use crate::{drivers::virtio::status, log};
use crate::drivers::virtio::transport::Transport;

// MMIO registers
const MMIO_MAGIC:               usize = 0x000;
const MMIO_VERSION:             usize = 0x004;
const MMIO_DEVICE_ID:           usize = 0x008;
const MMIO_VENDOR_ID:           usize = 0x00c;
const MMIO_DEVICE_FEATURES:     usize = 0x010;
const MMIO_DEVICE_FEATURES_SEL: usize = 0x014;
const MMIO_DRIVER_FEATURES:     usize = 0x020;
const MMIO_DRIVER_FEATURES_SEL: usize = 0x024;
const MMIO_QUEUE_SEL:           usize = 0x030;
const MMIO_QUEUE_NUM_MAX:       usize = 0x034;
const MMIO_QUEUE_NUM:           usize = 0x038;
const MMIO_QUEUE_READY:         usize = 0x044;
const MMIO_QUEUE_NOTIFY:        usize = 0x050;
const MMIO_INTERRUPT_STATUS:    usize = 0x060;
const MMIO_INTERRUPT_ACK:       usize = 0x064;
const MMIO_STATUS:              usize = 0x070;
const MMIO_QUEUE_DESC_LOW:      usize = 0x080;
const MMIO_QUEUE_DESC_HIGH:     usize = 0x084;
const MMIO_QUEUE_DRIVER_LOW:    usize = 0x090;
const MMIO_QUEUE_DRIVER_HIGH:   usize = 0x094;
const MMIO_QUEUE_DEVICE_LOW:    usize = 0x0a0;
const MMIO_QUEUE_DEVICE_HIGH:   usize = 0x0a4;
const MMIO_CONFIG_GENERATION:   usize = 0x0fc;
const MMIO_CONFIG:              usize = 0x100;

pub struct Mmio {
    pub base_addr: usize,
}

impl Mmio {
    pub fn magic(&self) -> u32 {
        self.read_u32(MMIO_MAGIC)
    }

    pub fn version(&self) -> u32 {
        self.read_u32(MMIO_VERSION)
    }

    #[inline]
    fn read_u8(&self, offset: usize) -> u8 {
        unsafe {
            core::ptr::read_volatile(
                (self.base_addr + offset) as *const u8,
            )
        }
    }

    #[inline]
    fn read_u16(&self, offset: usize) -> u16 {
        unsafe {
            core::ptr::read_volatile(
                (self.base_addr + offset) as *const u16,
            )
        }
    }

    #[inline]
    fn read_u32(&self, offset: usize) -> u32 {
        unsafe {
            core::ptr::read_volatile(
                (self.base_addr + offset) as *const u32,
            )
        }
    }

    #[inline]
    fn write_u32(&self, offset: usize, value: u32) {
        unsafe {
            core::ptr::write_volatile(
                (self.base_addr + offset) as *mut u32,
                value,
            )
        }
    }
}

impl Transport for Mmio {
    fn vendor_id(&self) -> u32 {
        self.read_u32(MMIO_VENDOR_ID)
    }

    fn device_id(&self) -> u32 {
        self.read_u32(MMIO_DEVICE_ID)
    }


    fn read_config_u8(&self, offset: usize) -> u8 {
        self.read_u8(MMIO_CONFIG + offset)
    }

    fn read_config_u16(&self, offset: usize) -> u16 {
        self.read_u16(MMIO_CONFIG + offset)
    }

    fn read_config_u32(&self, offset: usize) -> u32 {
        self.read_u32(MMIO_CONFIG + offset)
    }

    fn status(&self) -> u8 {
        self.read_u32(MMIO_STATUS) as u8
    }

    fn set_status(&mut self, status: u8) {
        self.write_u32(MMIO_STATUS, status as u32);
    }

    fn device_features(&mut self) -> u64 {
        self.write_u32(MMIO_DEVICE_FEATURES_SEL, 0);
        let lo = self.read_u32(MMIO_DEVICE_FEATURES) as u64;

        self.write_u32(MMIO_DEVICE_FEATURES_SEL, 1);
        let hi = self.read_u32(MMIO_DEVICE_FEATURES) as u64;

        (hi << 32) | lo

    }

    fn set_driver_features(&mut self, features: u64) {
        let lo = features as u32;
        let hi = (features >> 32) as u32;

        self.write_u32(MMIO_DRIVER_FEATURES_SEL, 0);
        self.write_u32(MMIO_DRIVER_FEATURES, lo);

        self.write_u32(MMIO_DRIVER_FEATURES_SEL, 1);
        self.write_u32(MMIO_DRIVER_FEATURES, hi);
    }

    fn select_queue(&mut self, index: u16) {
        self.write_u32(MMIO_QUEUE_SEL, index as u32);
    }

    fn queue_max_size(&self) -> u16 {
        self.read_u32(MMIO_QUEUE_NUM_MAX) as u16
    }

    fn set_queue_size(&mut self, size: u16) {
        self.write_u32(MMIO_QUEUE_NUM, size as u32);
    }

    fn set_queue_addresses(
        &mut self,
        desc_area_addr: u64,
        driver_area_addr: u64,
        device_area_addr: u64,
    ) {
        self.write_u32(MMIO_QUEUE_DESC_LOW, desc_area_addr as u32);
        self.write_u32(MMIO_QUEUE_DESC_HIGH, (desc_area_addr >> 32) as u32);

        self.write_u32(MMIO_QUEUE_DRIVER_LOW, driver_area_addr as u32);
        self.write_u32(MMIO_QUEUE_DRIVER_HIGH, (driver_area_addr >> 32) as u32);

        self.write_u32(MMIO_QUEUE_DEVICE_LOW, device_area_addr as u32);
        self.write_u32(MMIO_QUEUE_DEVICE_HIGH, (device_area_addr >> 32) as u32);
    }

    fn enable_queue(&mut self) {
        self.write_u32(MMIO_QUEUE_READY, 1);
    }

    fn notify_queue(&mut self, index: u16) {
        self.write_u32(MMIO_QUEUE_NOTIFY, index as u32);
    }
}


/*
pub struct Legacy {
    mmio_base: usize,
}

impl Legacy {
    pub fn new(mmio_base: usize) -> Self {

        macro_rules! read_u32 {
            ($offset:expr) => {{
                unsafe {
                    core::ptr::read_volatile(
                        (mmio_base + $offset) as *const u32,
                    ) 
                }
            }}
        }

        macro_rules! write_u32 {
            ($offset:expr, $value:expr) => {{
                unsafe {
                    core::ptr::write_volatile(
                        (mmio_base + $offset) as *mut u32,
                        $value,
                    ) 
                }
            }}
        }

        write_u32!(MMIO_STATUS, 0);
        write_u32!(MMIO_STATUS, STATUS_ACKNOWLEDGE);
        write_u32!(MMIO_STATUS, STATUS_ACKNOWLEDGE | STATUS_DRIVER);

        let device_features: u64 = {
            write_u32!(MMIO_DEVICE_FEATURES_SEL, 0);
            let lo = read_u32!(MMIO_DEVICE_FEATURES) as u64;

            write_u32!(MMIO_DEVICE_FEATURES_SEL, 1);
            let hi = read_u32!(MMIO_DEVICE_FEATURES) as u64;

            (hi << 32) | lo
        };

        log::uart_println!("device_features: 0x{device_features:016x}");

        Self { mmio_base }
    }


    fn read_u32(&self, off: usize) -> u32 {
        unsafe {
            core::ptr::read_volatile(
                (self.mmio_base + off) as *const u32,
            )
        }
    }

    fn write_u32(&self, off: usize, value: u32) {
        unsafe {
            core::ptr::write_volatile(
                (self.mmio_base + off) as *mut u32,
                value,
            );
        }
    }
}

*/

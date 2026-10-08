pub mod mmio;
pub use mmio::Mmio;

pub mod pci;
pub use pci::Pci;

pub trait Transport {
    fn vendor_id(&self) -> u32;
    fn device_id(&self) -> u32;

    fn status(&self) -> u8;
    fn set_status(&mut self, status: u8);

    fn device_features(&mut self) -> u64;
    fn set_driver_features(&mut self, features: u64);

    fn select_queue(&mut self, index: u16);
    fn queue_max_size(&self) -> u16;
    fn set_queue_size(&mut self, size: u16);

    fn set_queue_addresses(
        &mut self,
        desc_area_addr: u64,
        driver_area_addr: u64,
        device_area_addr: u64,
    );

    fn enable_queue(&mut self);
    fn notify_queue(&mut self, index: u16);

    fn read_config_u8(&self, offset: usize) -> u8;
    fn read_config_u16(&self, offset: usize) -> u16;
    fn read_config_u32(&self, offset: usize) -> u32;
    fn read_config_u64(&self, offset: usize) -> u64 {
        let lo = self.read_config_u32(offset) as u64;
        let hi = self.read_config_u32(offset + 4) as u64;
        lo | (hi << 32)
    }

    fn set_status_checked(&mut self, status: u8) -> Result<(), ()> {
        self.set_status(status);

        if self.status() == status {
            Ok(())
        } else {
            Err(())
        }
    }

    fn set_status_bits(&mut self, bits: u8) {
        let status = self.status() | bits;
        self.set_status(status);
    }
    
    fn set_status_bits_checked(&mut self, bits: u8) -> Result<(), ()> {
        let status = self.status() | bits;
        self.set_status(status);

        if self.status() == status {
            Ok(())
        } else {
            Err(())
        }
    }
}

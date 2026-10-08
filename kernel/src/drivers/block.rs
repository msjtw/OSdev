pub mod virtio_blk;

pub trait BlockDevice {
    type Error;
    
    fn block_size(&self) -> usize;
    fn block_count(&self) -> u64;

    fn read_block(
        &mut self,
        block_idx: u64,
        buf: &mut [u8],
    ) -> Result<(), Self::Error>;

    fn write_block(
        &mut self,
        block_idx: u64,
        buf: &[u8],
    ) -> Result<(), Self::Error>;

    fn flush(&mut self) -> Result<(), Self::Error>;
}

pub fn test() {
    fn _test(base: usize) -> bool {
        unsafe fn mmio_read_u32(base: usize, off: usize) -> u32 {
            unsafe {
                core::ptr::read_volatile((base + off) as *const u32)
            }
        }

        const TARGET_MAGIC: u32 = 0x74726976;
        const TARGET_DEVICE_ID: u32 = 2;
        const OFF_MAGIC: usize = 0x000;
        const OFF_DEVICE_ID: usize = 0x008;

        use crate::log;

        let magic = unsafe { mmio_read_u32(base, OFF_MAGIC) };
        if magic != TARGET_MAGIC {
            return false;
        }

        let device_id = unsafe { mmio_read_u32(base, OFF_DEVICE_ID) };
        if device_id != TARGET_DEVICE_ID {
            return false;
        }

        log::uart_println!("BLOCK DEVICE AT BASE=0x{base:08x}");
        log::uart_println!();
        let blk = virtio_blk::test(base);
        return true;
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

    for base in bases {
       _test(base);
    }
}

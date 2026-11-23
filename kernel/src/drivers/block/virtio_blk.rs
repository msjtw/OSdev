use blk_driver::virtio::transport::Mmio;
use blk_driver::virtio_blk::VirtioBlk as Driver;
pub use blk_driver::virtio_blk::{Operation, ReqHeader};
use blk_driver::{BlockDevice, virtio::Transport};

/// Temporary synchronous adapter for the still-active kernel filesystem.
pub struct VirtioBlk<T: Transport, const N: usize>(Driver<T, N>);

impl<const N: usize> VirtioBlk<Mmio, N> {
    pub fn new(base: usize) -> Result<Self, ()> {
        let region = crate::process::blk_dma::Reservation::reserve()?.identity_region()?;
        Ok(Self(unsafe { Driver::from_workspace(base, region)? }))
    }
    pub fn init(&mut self) -> Result<(), ()> {
        self.0.init()
    }
}

impl<T: Transport, const N: usize> VirtioBlk<T, N> {
    fn wait(&mut self) -> Result<(), ()> {
        while self.0.poll_complete()?.is_none() {
            core::hint::spin_loop();
        }
        Ok(())
    }
}

impl<T: Transport, const N: usize> BlockDevice for VirtioBlk<T, N> {
    type Error = ();
    fn block_size(&self) -> usize {
        self.0.block_size()
    }
    fn block_count(&self) -> u64 {
        self.0.block_count()
    }
    fn read_block(&mut self, index: u64, bytes: &mut [u8]) -> Result<(), ()> {
        if bytes.len() != self.block_size() {
            return Err(());
        }
        self.0.submit_read(index)?;
        self.wait()?;
        bytes.copy_from_slice(self.0.read_data()?);
        Ok(())
    }
    fn write_block(&mut self, index: u64, bytes: &[u8]) -> Result<(), ()> {
        self.0.submit_write(index, bytes)?;
        self.wait()
    }
    fn flush(&mut self) -> Result<(), ()> {
        self.0.submit_flush()?;
        self.wait()
    }
}

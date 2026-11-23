#![no_std]

extern crate alloc;

pub mod virtio;
pub mod dma;
pub mod virtio_blk;
mod log;
pub use log::{Level, set_logger};

pub trait BlockDevice {
    type Error;
    fn block_size(&self) -> usize;
    fn block_count(&self) -> u64;
    fn read_block(&mut self, index: u64, buffer: &mut [u8]) -> Result<(), Self::Error>;
    fn write_block(&mut self, index: u64, buffer: &[u8]) -> Result<(), Self::Error>;
    fn flush(&mut self) -> Result<(), Self::Error>;
}

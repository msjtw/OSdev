use core::mem::{align_of, size_of};
use crate::{virtio::SplitQueue, virtio_blk::ReqHeader};

pub const PAGE_SIZE: usize = 4096;
pub const DEFAULT_BOUNCE_CAPACITY: usize = 4096;

fn align(value: usize, alignment: usize) -> Option<usize> {
    value.checked_add(alignment - 1).map(|n| n & !(alignment - 1))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub queue: usize,
    pub header: usize,
    pub status: usize,
    pub bounce: usize,
    pub bounce_capacity: usize,
    pub size: usize,
}

impl Layout {
    pub fn new(bounce_capacity: usize) -> Option<Self> {
        if bounce_capacity < 512 || !bounce_capacity.is_power_of_two() { return None; }
        let header = align(size_of::<SplitQueue<8>>(), align_of::<ReqHeader>())?;
        let status = header.checked_add(size_of::<ReqHeader>())?;
        let bounce = align(status.checked_add(1)?, PAGE_SIZE)?;
        let size = align(bounce.checked_add(bounce_capacity)?, PAGE_SIZE)?;
        Some(Self { queue: 0, header, status, bounce, bounce_capacity, size })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub virtual_base: usize,
    pub physical_base: usize,
    pub size: usize,
}

impl Region {
    pub fn new(virtual_base: usize, physical_base: usize, size: usize, layout: Layout) -> Option<Self> {
        if virtual_base == 0 || physical_base == 0 || size < layout.size || size == 0
            || virtual_base % PAGE_SIZE != 0 || physical_base % PAGE_SIZE != 0 || size % PAGE_SIZE != 0 {
            return None;
        }
        virtual_base.checked_add(size)?;
        physical_base.checked_add(size)?;
        Some(Self { virtual_base, physical_base, size })
    }

    pub fn device_address(self, address: usize, length: usize) -> Option<u64> {
        let offset = address.checked_sub(self.virtual_base)?;
        if offset.checked_add(length)? > self.size { return None; }
        Some(self.physical_base.checked_add(offset)? as u64)
    }
}

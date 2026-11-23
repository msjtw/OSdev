//! Boot-only reservation. No Drop: device-visible RAM survives process teardown.
use crate::{HEAP_ALLOCATOR, process::Process};
use blk_driver::dma::{DEFAULT_BOUNCE_CAPACITY, Layout as WorkspaceLayout, PAGE_SIZE, Region};
use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Once;

static RESERVED: Once<Reservation> = Once::new();

pub struct Reservation {
    physical_base: usize,
    size: usize,
    claimed: AtomicBool,
}

impl Reservation {
    pub fn identity_region(&self) -> Result<Region, ()> {
        if self.claimed.swap(true, Ordering::AcqRel) {
            return Err(());
        }
        Ok(Region {
            virtual_base: self.physical_base,
            physical_base: self.physical_base,
            size: self.size,
        })
    }
    /// Reserve the single boot workspace; intentionally never return it to the heap.
    pub fn reserve() -> Result<&'static Self, ()> {
        RESERVED.try_call_once(|| {
            let workspace = WorkspaceLayout::new(DEFAULT_BOUNCE_CAPACITY).ok_or(())?;
            let layout = Layout::from_size_align(workspace.size, PAGE_SIZE).map_err(|_| ())?;
            let pointer = unsafe { HEAP_ALLOCATOR.alloc_zeroed(layout) };
            if pointer.is_null() {
                return Err(());
            }
            Ok(Self {
                physical_base: pointer as usize,
                size: workspace.size,
                claimed: AtomicBool::new(false),
            })
        })
    }

    /// Map only reserved external pages; they are never owned by the process.
    pub fn map_into(&self, proc: &mut Process, virtual_base: usize) -> Result<Region, ()> {
        let layout = WorkspaceLayout::new(DEFAULT_BOUNCE_CAPACITY).ok_or(())?;
        let region = Region::new(virtual_base, self.physical_base, self.size, layout).ok_or(())?;
        if self.claimed.swap(true, Ordering::AcqRel) {
            return Err(());
        }
        if proc
            .pagetable
            .map_blk_dma(virtual_base, self.physical_base, self.size)
            .is_err()
        {
            self.claimed.store(false, Ordering::Release);
            return Err(());
        }
        Ok(region)
    }
}

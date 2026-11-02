use buddy_system_allocator::LockedHeap;
use core::alloc::{GlobalAlloc, Layout};

use crate::{Error, Result, runtime::HEAP_ALIGNMENT};

pub struct FixedHeap {
    heap: LockedHeap<{ usize::BITS as usize }>,
}

impl FixedHeap {
    pub const fn new() -> Self {
        Self {
            heap: LockedHeap::empty(),
        }
    }

    pub unsafe fn init(&self, start: usize, size: usize) -> Result<()> {
        if start == 0
            || size == 0
            || start % HEAP_ALIGNMENT != 0
            || size % HEAP_ALIGNMENT != 0
            || start.checked_add(size).is_none()
        {
            return Err(Error::EINVAL);
        }
        let mut heap = self.heap.lock();
        if heap.stats_total_bytes() != 0 {
            return Err(Error::EINVAL);
        }
        unsafe { heap.init(start, size) };
        Ok(())
    }
}

impl Default for FixedHeap {
    fn default() -> Self {
        Self::new()
    }
}

unsafe impl GlobalAlloc for FixedHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { self.heap.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { self.heap.dealloc(ptr, layout) }
    }
}

#[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))]
#[alloc_error_handler]
fn allocation_failure(_: Layout) -> ! {
    crate::syscall::exit(crate::runtime::EXIT_FAILURE)
}

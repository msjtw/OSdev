// Modified LockedHeap from rcore-os/buddy_system_allocator

use buddy_system_allocator::Heap;
use core::alloc::GlobalAlloc;

use core::alloc::Layout;
use core::ops::Deref;
use core::ptr::NonNull;

use crate::debug;
use crate::lock::IntMutex;
use alloc::alloc::{AllocError, Allocator};

use crate::{
    HEAP_ALLOCATOR,
    virtmemory::{PAGE_LAYOUT, PAGESIZE},
};

#[derive(Default)]
pub struct LockedHeap<const ORDER: usize>(IntMutex<Heap<ORDER>>);

impl<const ORDER: usize> LockedHeap<ORDER> {
    pub const fn new() -> Self {
        LockedHeap(IntMutex::new(Heap::<ORDER>::new()))
    }

    pub const fn empty() -> Self {
        LockedHeap(IntMutex::new(Heap::<ORDER>::new()))
    }
}

impl<const ORDER: usize> Deref for LockedHeap<ORDER> {
    type Target = IntMutex<Heap<ORDER>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

unsafe impl<const ORDER: usize> GlobalAlloc for LockedHeap<ORDER> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.0
            .lock()
            .alloc(layout)
            .ok()
            .map_or(core::ptr::null_mut(), |allocation| allocation.as_ptr())
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe {
            let ptr_non = NonNull::new(ptr).unwrap();
            if ptr == 0x80235000 as *mut u8 {
                debug!("im in");
            }
            self.0.lock().dealloc(ptr_non, layout);
        }
    }
}

pub struct FrameAllocator {}

unsafe impl Allocator for FrameAllocator {
    fn allocate(
        &self,
        layout: core::alloc::Layout,
    ) -> Result<core::ptr::NonNull<[u8]>, alloc::alloc::AllocError> {
        if layout.size() > PAGESIZE {
            return Err(AllocError);
        }
        let frame_ptr = unsafe { HEAP_ALLOCATOR.alloc(PAGE_LAYOUT) };

        let ptr = core::ptr::NonNull::new(frame_ptr).ok_or(alloc::alloc::AllocError)?;

        let slice = core::ptr::NonNull::slice_from_raw_parts(ptr, layout.size());

        Ok(slice)
    }

    unsafe fn deallocate(&self, ptr: core::ptr::NonNull<u8>, _layout: core::alloc::Layout) {
        unsafe {
            HEAP_ALLOCATOR.dealloc(ptr.as_ptr(), PAGE_LAYOUT);
        }
    }
}

// Modified LockedHeap from rcore-os/buddy_system_allocator

use buddy_system_allocator::Heap;
use core::alloc::GlobalAlloc;

extern crate spin;

use core::alloc::Layout;
use core::ops::Deref;
use core::ptr::NonNull;
use spin::Mutex;

use alloc::alloc::{AllocError, Allocator};

use crate::{
    HEAP_ALLOCATOR,
    virtmemory::{PAGE_LAYOUT, PAGESIZE},
};

#[derive(Default)]
pub struct LockedHeap<const ORDER: usize>(Mutex<Heap<ORDER>>);

impl<const ORDER: usize> LockedHeap<ORDER> {
    pub const fn new() -> Self {
        LockedHeap(Mutex::new(Heap::<ORDER>::new()))
    }

    pub const fn empty() -> Self {
        LockedHeap(Mutex::new(Heap::<ORDER>::new()))
    }
}

impl<const ORDER: usize> Deref for LockedHeap<ORDER> {
    type Target = Mutex<Heap<ORDER>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

unsafe impl<const ORDER: usize> GlobalAlloc for LockedHeap<ORDER> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe {
            (*crate::CPU).push_interrupt_off();
            let res = self
                .0
                .lock()
                .alloc(layout)
                .ok()
                .map_or(core::ptr::null_mut(), |allocation| allocation.as_ptr());
            (*crate::CPU).pop_interrupt_off();
            res
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe {
            (*crate::CPU).push_interrupt_off();
            self.0.lock().dealloc(NonNull::new_unchecked(ptr), layout);
            (*crate::CPU).pop_interrupt_off();
        }
    }
}

pub struct FrameAllocator {}

unsafe impl Allocator for FrameAllocator {
    fn allocate(
        &self,
        layout: core::alloc::Layout,
    ) -> Result<core::ptr::NonNull<[u8]>, alloc::alloc::AllocError> {
        if layout.size() > PAGESIZE as usize {
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

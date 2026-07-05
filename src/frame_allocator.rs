use core::alloc::GlobalAlloc;

use alloc::{
    alloc::{AllocError, Allocator},
    format,
};

use crate::{
    HEAP_ALLOCATOR, print,
    virtmemory::{PAGE_LAYOUT, PAGESIZE},
};

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
        // print!("aaaa 0x{:x}\n", frame_ptr as u32);

        let slice = core::ptr::NonNull::slice_from_raw_parts(ptr, layout.size());

        Ok(slice)
    }

    unsafe fn deallocate(&self, ptr: core::ptr::NonNull<u8>, _layout: core::alloc::Layout) {
        unsafe {
            HEAP_ALLOCATOR.dealloc(ptr.as_ptr(), PAGE_LAYOUT);
        }
    }
}

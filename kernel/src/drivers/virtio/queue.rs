use core::ptr;
use core::sync::atomic::fence;
use core::sync::atomic::Ordering;

#[repr(C, align(16))]
pub struct DescArea<const N: usize> {
    pub entries: [Desc; N],
}

#[repr(C, align(2))]
pub struct DriverArea<const N: usize> {
    pub flags: u16,
    pub idx: u16,
    pub ring: [u16; N],

    // pub avail_event: u16,
}

#[repr(C, align(4))]
pub struct DeviceArea<const N: usize> {
    pub flags: u16,
    pub idx: u16,
    pub ring: [DeviceAreaElem; N],

    // pub avail_event: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DeviceAreaElem {
    pub id: u32,
    pub len: u32,
}


#[repr(C)]
pub struct SplitQueue<const N: usize> {
    pub desc_area: DescArea<N>,
    pub driver_area: DriverArea<N>,
    pub device_area: DeviceArea<N>,

    pub last_dev_area_idx: u16,
}

impl<const N: usize> SplitQueue<N> {
    pub const fn new() -> Self {
        const EMPTY_DEV_AREA_ELEM: DeviceAreaElem = DeviceAreaElem {
            id: 0,
            len: 0,
        };

        Self {
            desc_area: DescArea {
                entries: [Desc::new(); N],
            },

            driver_area: DriverArea {
                flags: 0,
                idx: 0,
                ring: [0; N],
            },

            device_area: DeviceArea {
                flags: 0,
                idx: 0,
                ring: [EMPTY_DEV_AREA_ELEM; N],
            },

            last_dev_area_idx: 0,
        }
    }

    pub fn size(&self) -> u16 {
        N as u16
    }

    pub fn desc_area_addr(&self) -> u64 {
        &self.desc_area as *const _ as usize as u64
    }

    pub fn driver_area_addr(&self) -> u64 {
        &self.driver_area as *const _ as usize as u64
    }

    pub fn device_area_addr(&self) -> u64 {
        &self.device_area as *const _ as usize as u64
    }

    pub fn set_desc(&mut self, idx: usize, desc: Desc) {
        self.desc_area.entries[idx] = desc;
    }

    pub fn desc(&mut self, idx: usize) -> DescBuilder<'_> {
        self.desc_area.entries[idx] = Desc::new();
        DescBuilder { desc: &mut self.desc_area.entries[idx] }
    }

    pub fn push(&mut self, head: u16) {
        let idx = self.driver_area.idx;
        let slot = idx as usize % N;

        unsafe {
            ptr::write_volatile(
                &mut self.driver_area.ring[slot],
                head,
            );
        }

        fence(Ordering::Release);

        unsafe {
            ptr::write_volatile(
                &mut self.driver_area.idx,
                idx.wrapping_add(1),
            );
        }
    }

    pub fn pop_optional(&mut self) -> Option<DeviceAreaElem> {
        let device_idx = unsafe {
            ptr::read_volatile(&self.device_area.idx)
        };

        if device_idx == self.last_dev_area_idx {
            return None;
        }

        fence(Ordering::Acquire);

        let slot = self.last_dev_area_idx as usize % N;
        
        let elem = unsafe {
            ptr::read_volatile(&self.device_area.ring[slot])
        };

        self.last_dev_area_idx = self.last_dev_area_idx.wrapping_add(1);

        Some(elem)
    }
}

const FLAG_NEXT: u16 = 1;
const FLAG_WRITE: u16 = 2;
const FLAG_INDIRECT: u16 = 4;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Desc {
    pub addr: u64,
    pub len: u32,
    pub flags: u16,
    pub next: u16,
}

impl Desc {
    const fn new() -> Self {
        Self {
            addr: 0,
            len: 0,
            flags: 0,
            next: 0,
        }
    }
}

pub struct DescBuilder<'a> {
    desc: &'a mut Desc,
}

impl<'a> DescBuilder<'a> {
    pub fn addr<T: ?Sized>(mut self, value: &T) -> Self {
        self.desc.addr =
            core::ptr::from_ref(value)
                .cast::<u8>() as usize as u64;
        self
    }

    #[inline]
    pub fn addr_raw(mut self, value: usize) -> Self {
        self.desc.addr = value as u64;
        self
    }
    
    #[inline]
    pub fn len(mut self, value: usize) -> Self {
        self.desc.len = value as u32;
        self
    }
    
    #[inline]
    pub fn len_from_type<T>(mut self) -> Self {
        self.desc.len = core::mem::size_of::<T>() as u32;
        self
    }

    #[inline]
    pub fn next(mut self, value: u16) -> Self {
        self.desc.flags |= FLAG_NEXT;
        self.desc.next = value;
        self
    }

    #[inline]
    pub fn flag_write(mut self) -> Self {
        self.desc.flags |= FLAG_WRITE;
        self
    }

    #[inline]
    pub fn flag_indirect(mut self) -> Self {
        self.desc.flags |= FLAG_INDIRECT;
        self
    }
}

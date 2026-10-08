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
pub struct Desc {
    pub addr: u64,
    pub len: u32,
    pub flags: u16,
    pub next: u16,
}


#[repr(C)]
#[derive(Clone, Copy)]
pub struct DeviceAreaElem {
    id: u32,
    len: u32,
}


pub struct Queue<const N: usize> {
    pub desc_area: DescArea<N>,
    pub driver_area: DriverArea<N>,
    pub device_area: DeviceArea<N>,
}

impl<const N: usize> Queue<N> {
    pub const fn new() -> Self {
        const EMPTY_DESC: Desc = Desc {
            addr: 0,
            len: 0,
            flags: 0,
            next: 0,
        };

        const EMPTY_DEV_AREA_ELEM: DeviceAreaElem = DeviceAreaElem {
            id: 0,
            len: 0,
        };

        Self {
            desc_area: DescArea {
                entries: [EMPTY_DESC; N],
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
            }
        }
    }

    fn size(&self) -> u16 {
        N as u16
    }

    fn desc_area_addr(&self) -> u64 {
        &self.desc_area as *const _ as usize as u64
    }

    fn driver_area_addr(&self) -> u64 {
        &self.driver_area as *const _ as usize as u64
    }

    fn device_area_addr(&self) -> u64 {
        &self.device_area as *const _ as usize as u64
    }

    fn set_desc(&mut self, idx: usize, desc: Desc) {
        self.desc_area.entries[idx] = desc;
    }

    
}

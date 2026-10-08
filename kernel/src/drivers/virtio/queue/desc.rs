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

pub struct DescBuilder {
}

impl Desc {
    pub const fn new() -> Self {
        Self {
            addr: 0,
            len: 0,
            flags: 0,
            next: 0,
        }
    }

    pub const fn addr<T>(mut self, value: &T) -> Self {
        self.addr = value as *const T as usize as u64;
        self
    }

    pub const fn addr_raw(mut self, value: usize) -> Self {
        self.addr = value as u64;
        self
    }
    
    pub const fn len(mut self, value: u32) -> Self {
        self.len = value;
        self
    }
    
    pub const fn len_from_type<T>(mut self) -> Self {
        self.len = core::mem::size_of::<T>();
        self
    }

    pub const fn next(mut self, value: u16) -> Self {
        self.flags |= FLAG_NEXT;
        self.next = value;
        self
    }

    pub const fn flag_write(mut self) -> Self {
        self.flags |= FLAG_WRITE;
        self
    }

    pub const fn flag_indirect(mut self) -> Self {
        self.flags |= FLAG_INDIRECT;
        self
    }
}

pub struct DescBuilder<'a> {
    desc: &'a mut Desc,
}

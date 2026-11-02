use core::ptr::{read_volatile, write_volatile};

const LSR: usize = 5;
const DATA_READY: u8 = 1;
const TRANSMIT_READY: u8 = 1 << 5;

pub struct Uart {
    base: *mut u8,
}

impl Uart {
    pub unsafe fn new(address: usize) -> Self {
        Self {
            base: address as *mut u8,
        }
    }

    pub fn data_ready(&self) -> bool {
        unsafe { read_volatile(self.base.add(LSR)) & DATA_READY != 0 }
    }

    pub fn read_byte(&self) -> u8 {
        unsafe { read_volatile(self.base) }
    }

    pub fn write(&self, bytes: &[u8]) {
        for &byte in bytes {
            while unsafe { read_volatile(self.base.add(LSR)) & TRANSMIT_READY == 0 } {
                core::hint::spin_loop();
            }
            unsafe { write_volatile(self.base, byte) };
        }
    }
}

use core::ptr::write_volatile;

use crate::virtmemory::UART;

pub static UART_DRIVER: UartDriver = UartDriver::new();

#[derive(Debug, Default)]
pub struct UartDriver;

impl UartDriver {
    pub const fn new() -> Self {
        Self
    }

    pub fn write(&self, bytes: &[u8]) {
        let uart = UART as *mut u8;
        for &byte in bytes {
            unsafe { write_volatile(uart, byte) };
        }
    }
}

// Heap-free formatting buffer for use in trap/interrupt context, where heap
// allocation is unsafe (it would corrupt interrupt_prev_state via IntMutex).
pub struct StackFormatter {
    buf: [u8; 256],
    pos: usize,
}

impl StackFormatter {
    pub const fn new() -> Self {
        Self {
            buf: [0u8; 256],
            pos: 0,
        }
    }

    pub fn flush(&self) {
        UART_DRIVER.write(&self.buf[..self.pos]);
    }
}

impl core::fmt::Write for StackFormatter {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for b in s.bytes() {
            if self.pos < self.buf.len() {
                self.buf[self.pos] = b;
                self.pos += 1;
            }
        }
        Ok(())
    }
}

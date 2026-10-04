use core::ptr::write_volatile;

use crate::virtmemory::UART;

pub fn uart_write(bytes: &[u8]) {
    let uart = UART as *mut u8;
    for &byte in bytes {
        unsafe { write_volatile(uart, byte) };
    }
}

// Stack-allocated writer for use in trap/interrupt context where heap
// allocation is unsafe (would corrupt interrupt_prev_state via IntMutex).
pub struct UartWriter {
    buf: [u8; 256],
    pos: usize,
}

impl UartWriter {
    pub const fn new() -> Self {
        Self {
            buf: [0u8; 256],
            pos: 0,
        }
    }

    pub fn flush(&self) {
        let uart = UART as *mut u8;
        for &b in &self.buf[..self.pos] {
            unsafe { write_volatile(uart, b) };
        }
    }
}

impl core::fmt::Write for UartWriter {
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

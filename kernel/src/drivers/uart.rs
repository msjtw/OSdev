use core::ptr::{read_volatile, write_volatile};

use crate::virtmemory::UART;

const UART_RHR: usize = 0;
const UART_LSR: usize = 5;
const UART_LSR_DATA_READY: u8 = 1 << 0;

pub static UART_DRIVER: UartDriver = UartDriver::new();

#[derive(Debug, Default)]
pub struct UartDriver;

impl UartDriver {
    pub const fn new() -> Self {
        Self
    }

    /// Drain raw bytes from the UART receive FIFO.
    pub fn drain_input(&self, mut consume: impl FnMut(u8)) {
        let uart = UART as *const u8;

        // Reading the receiver holding register consumes one byte. Drain all
        // bytes queued in the UART receive FIFO before this tick ends.
        while unsafe { read_volatile(uart.add(UART_LSR)) } & UART_LSR_DATA_READY != 0 {
            consume(unsafe { read_volatile(uart.add(UART_RHR)) });
        }
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

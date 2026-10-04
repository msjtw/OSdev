use core::ptr::{read_volatile, write_volatile};

use crate::{KERNEL, kernel::STDIN_CHANNEL, virtmemory::UART};

pub const INPUT_QUEUE_CAPACITY: usize = 256;

const UART_RHR: usize = 0;
const UART_LSR: usize = 5;
const UART_LSR_DATA_READY: u8 = 1 << 0;

pub fn uart_write(bytes: &[u8]) {
    let uart = UART as *mut u8;
    for &byte in bytes {
        unsafe { write_volatile(uart, byte) };
    }
}

pub fn uart_input_poll() {
    let uart = UART as *const u8;
    let mut received = false;
    let mut kernel = KERNEL.get().unwrap().lock();

    // Reading the receiver holding register consumes one byte. Drain all
    // bytes queued in the UART receive FIFO before returning from this tick.
    while unsafe { read_volatile(uart.add(UART_LSR)) } & UART_LSR_DATA_READY != 0 {
        let byte = unsafe { read_volatile(uart.add(UART_RHR)) };
        received |= kernel.input_queue.push(byte);
    }

    if received {
        kernel.wakeup(Some(STDIN_CHANNEL), false);
    }
}

/// Fixed-size FIFO for bytes received from the UART.
///
/// It is kept in `Kernel` so timer-side polling and `read(fd = 0)` share one
/// input stream without allocating in an interrupt handler.
pub struct InputQueue {
    bytes: [u8; INPUT_QUEUE_CAPACITY],
    head: usize,
    len: usize,
}

impl InputQueue {
    pub const fn new() -> Self {
        Self {
            bytes: [0; INPUT_QUEUE_CAPACITY],
            head: 0,
            len: 0,
        }
    }

    pub fn push(&mut self, byte: u8) -> bool {
        if self.len == INPUT_QUEUE_CAPACITY {
            return false;
        }

        let tail = (self.head + self.len) % INPUT_QUEUE_CAPACITY;
        self.bytes[tail] = byte;
        self.len += 1;
        true
    }

    pub fn pop(&mut self) -> Option<u8> {
        if self.len == 0 {
            return None;
        }

        let byte = self.bytes[self.head];
        self.head = (self.head + 1) % INPUT_QUEUE_CAPACITY;
        self.len -= 1;
        Some(byte)
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Default for InputQueue {
    fn default() -> Self {
        Self::new()
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

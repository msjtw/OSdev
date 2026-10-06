use core::ptr::{read_volatile, write_volatile};

use alloc::vec::Vec;

use crate::{
    KERNEL,
    kernel::STDIN_CHANNEL,
    lock::IntMutex,
    process::{ProcState, Process},
    structures::RingBuffer,
    virtmemory::UART,
};

pub const INPUT_QUEUE_CAPACITY: usize = 256;

const UART_RHR: usize = 0;
const UART_LSR: usize = 5;
const UART_LSR_DATA_READY: u8 = 1 << 0;

pub static UART_DRIVER: UartDriver = UartDriver::new();

#[derive(Debug, Default)]
pub struct UartDriver {
    input: IntMutex<RingBuffer<INPUT_QUEUE_CAPACITY>>,
}

impl UartDriver {
    pub const fn new() -> Self {
        Self {
            input: IntMutex::new(RingBuffer::new()),
        }
    }

    pub fn poll_input(&self) {
        let uart = UART as *const u8;
        let mut received = false;

        {
            let mut input = self.input.lock();

            // Reading the receiver holding register consumes one byte. Drain
            // all bytes queued in the UART receive FIFO before this tick ends.
            while unsafe { read_volatile(uart.add(UART_LSR)) } & UART_LSR_DATA_READY != 0 {
                let byte = unsafe { read_volatile(uart.add(UART_RHR)) };
                received |= input.push(byte);
            }
        }

        if received {
            KERNEL
                .get()
                .unwrap()
                .lock()
                .wakeup(Some(STDIN_CHANNEL), false);
        }
    }

    /// Return available input, sleeping until at least one byte arrives.
    ///
    /// The empty check and sleeping-state transition occur under the input
    /// lock. This prevents timer polling from adding input between them and
    /// losing the corresponding wakeup.
    pub fn read_blocking(&self, proc: &mut Process, len: usize) -> Vec<u8> {
        loop {
            let mut bytes = Vec::new();
            let sleeping;

            {
                let mut input = self.input.lock();
                let requested = len.min(INPUT_QUEUE_CAPACITY);
                while bytes.len() < requested {
                    let Some(byte) = input.pop() else {
                        break;
                    };
                    bytes.push(byte);
                }

                sleeping = bytes.is_empty();
                if sleeping {
                    unsafe { proc.lock.lock_manual() };
                    proc.sleep_channel = Some(STDIN_CHANNEL);
                    proc.state = ProcState::Sleeping;
                }
            }

            if !sleeping {
                return bytes;
            }

            // The UART input lock has dropped, so timer polling can enqueue
            // input and wake this process while it is scheduled out.
            unsafe { proc.sleep_locked() };
        }
    }

    pub fn write(&self, bytes: &[u8]) {
        let uart = UART as *mut u8;
        for &byte in bytes {
            unsafe { write_volatile(uart, byte) };
        }
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

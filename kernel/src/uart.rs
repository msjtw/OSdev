use core::ptr::{read_volatile, write_volatile};

use crate::{
    KERNEL,
    kernel::STDIN_CHANNEL,
    lock::IntMutex,
    process::{ProcState, Process, fd::{Errno, FileOps}},
    structures::RingBuffer,
    virtmemory::{UART, copy_in_cont, copy_out_cont},
};

pub const INPUT_QUEUE_CAPACITY: usize = 256;

const UART_RHR: usize = 0;
const UART_LSR: usize = 5;
const UART_LSR_DATA_READY: u8 = 1 << 0;

#[derive(Debug)]
pub struct UartTerminal {
    input: IntMutex<RingBuffer<INPUT_QUEUE_CAPACITY>>,
}

pub static UART_TERMINAL: UartTerminal = UartTerminal::new();

impl UartTerminal {
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
            KERNEL.get().unwrap().lock().wakeup(Some(STDIN_CHANNEL), false);
        }
    }
}

impl FileOps for UartTerminal {
    fn read(&self, proc: &mut Process, addr: usize, len: usize) -> Result<usize, Errno> {
        if len == 0 {
            return Ok(0);
        }

        let mut bytes = [0u8; INPUT_QUEUE_CAPACITY];

        loop {
            let mut count = 0;
            let mut sleeping = false;

            {
                // Keep queue consumption and publication of the sleeping
                // state atomic with respect to timer-side UART polling.
                let mut input = self.input.lock();
                let requested = len.min(bytes.len());
                while count < requested {
                    let Some(byte) = input.pop() else {
                        break;
                    };
                    bytes[count] = byte;
                    count += 1;
                }

                if count == 0 {
                    unsafe { proc.lock.lock_manual() };
                    proc.sleep_channel = Some(STDIN_CHANNEL);
                    proc.state = ProcState::Sleeping;
                    sleeping = true;
                }
            }

            if count != 0 {
                return copy_out_cont(&mut proc.pagetable, addr, &bytes[..count])
                    .map(|()| count)
                    .map_err(|()| Errno::Fault);
            }

            if sleeping {
                unsafe { proc.sleep_locked() };
            }
        }
    }

    fn write(&self, proc: &mut Process, addr: usize, len: usize) -> Result<usize, Errno> {
        let bytes = copy_in_cont(&mut proc.pagetable, addr, len).map_err(|()| Errno::Fault)?;
        uart_write(&bytes);
        Ok(bytes.len())
    }

    fn ioctl(&self, proc: &mut Process, op: usize, arg: usize) -> Result<usize, Errno> {
        const TIOCGWINSZ: usize = 0x5413;
        if op != TIOCGWINSZ {
            return Err(Errno::NotATerminal);
        }

        // struct winsize { unsigned short ws_row, ws_col, ws_xpixel, ws_ypixel; }
        let mut winsize = [0u8; 8];
        winsize[0..2].copy_from_slice(&24u16.to_ne_bytes());
        winsize[2..4].copy_from_slice(&80u16.to_ne_bytes());
        copy_out_cont(&mut proc.pagetable, arg, &winsize)
            .map(|()| 0)
            .map_err(|()| Errno::Fault)
    }
}

pub fn uart_write(bytes: &[u8]) {
    let uart = UART as *mut u8;
    for &byte in bytes {
        unsafe { write_volatile(uart, byte) };
    }
}

pub fn uart_input_poll() {
    UART_TERMINAL.poll_input();
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

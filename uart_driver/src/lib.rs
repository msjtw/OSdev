#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use serverlib::{
    Error, Result,
    abi::{KERNEL_SENDER_ID, Message},
};

pub mod service;
pub mod uart;

/// Validate stamped sender identity before any operation state or hardware access.
pub(crate) fn authorize(message: &Message, tty_pid: Option<usize>) -> Result<()> {
    if message.operation == serverlib::uart::POLL {
        return if message.sender == KERNEL_SENDER_ID {
            Ok(())
        } else {
            Err(Error::EPERM)
        };
    }
    if message.sender == KERNEL_SENDER_ID || Some(message.sender) == tty_pid {
        Ok(())
    } else {
        Err(Error::EPERM)
    }
}

/// Drain currently available bytes, reserving before consuming and returning partial progress.
pub(crate) fn drain(uart: &uart::Uart, count: usize, buffer: &mut Vec<u8>) -> Result<usize> {
    buffer.clear();
    while buffer.len() < count && uart.data_ready() {
        if buffer.try_reserve(1).is_err() {
            return if buffer.is_empty() {
                Err(Error::ENOMEM)
            } else {
                Ok(buffer.len())
            };
        }
        // Reserve before consuming a byte so exhaustion cannot lose input.
        buffer.push(uart.read_byte());
    }
    Ok(buffer.len())
}

#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use serverlib::{
    Error, Result,
    abi::{KERNEL_SENDER_ID, Message, SYS_READ, SYS_WRITE},
};

pub mod uart;

pub fn handle(
    message: &Message,
    uart: &uart::Uart,
    buffer: &mut Vec<u8>,
    read_payload: impl FnOnce(&mut [u8]) -> Result<usize>,
) -> Result<usize> {
    buffer.clear();
    if message.sender != KERNEL_SENDER_ID {
        return Err(Error::EPERM);
    }
    let count = message.args[2];
    match message.operation {
        SYS_READ => {
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
        SYS_WRITE => {
            if count == 0 {
                return Ok(0);
            }
            buffer.try_reserve_exact(count).map_err(|_| Error::ENOMEM)?;
            buffer.resize(count, 0);
            if read_payload(buffer)? != count {
                return Err(Error::EIO);
            }
            // Do not transmit anything until the complete payload has been copied.
            uart.write(buffer);
            Ok(count)
        }
        _ => Err(Error::ENOSYS),
    }
}

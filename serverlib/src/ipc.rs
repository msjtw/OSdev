use crate::{Error, Result, abi, error::decode};

pub use abi::{KERNEL_SENDER_ID, Message};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub id: usize,
    pub message: Message,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Request(Request),
    Completion { id: usize, result: Result<usize> },
}

impl TryFrom<abi::IpcEvent> for Event {
    type Error = Error;

    fn try_from(event: abi::IpcEvent) -> Result<Self> {
        match event.kind {
            abi::IPC_EVENT_REQUEST => Ok(Self::Request(Request {
                id: event.id,
                message: event.message,
            })),
            abi::IPC_EVENT_COMPLETION => Ok(Self::Completion {
                id: event.id,
                result: decode(event.result),
            }),
            _ => Err(Error::EINVAL),
        }
    }
}

#[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))]
mod transport;
#[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))]
pub use transport::*;

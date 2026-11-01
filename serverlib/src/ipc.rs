use crate::{
    Result, abi,
    error::{decode, encode},
    syscall::syscall,
};

pub use abi::{KERNEL_SENDER_ID, Message};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub id: usize,
    pub message: Message,
}

pub unsafe fn call(destination: usize, message: &Message) -> Result<usize> {
    decode(unsafe {
        syscall(
            abi::SYS_IPC_CALL,
            [destination, message as *const Message as usize, 0, 0, 0, 0],
        )
    })
}

pub fn receive() -> Result<Request> {
    let mut message = Message::default();
    let id = decode(unsafe {
        syscall(
            abi::SYS_IPC_RECEIVE,
            [&mut message as *mut Message as usize, 0, 0, 0, 0, 0],
        )
    })?;
    Ok(Request { id, message })
}

pub fn read_payload(id: usize, offset: usize, destination: &mut [u8]) -> Result<usize> {
    decode(unsafe {
        syscall(
            abi::SYS_IPC_READ_PAYLOAD,
            [
                id,
                offset,
                destination.as_mut_ptr() as usize,
                destination.len(),
                0,
                0,
            ],
        )
    })
}

pub fn reply(id: usize, result: Result<usize>, data: Option<&[u8]>) -> Result<()> {
    let (source, length) = match data {
        Some(bytes) => (bytes.as_ptr() as usize, bytes.len()),
        None => (0, 0),
    };
    decode(unsafe {
        syscall(
            abi::SYS_IPC_REPLY,
            [id, encode(result), source, length, 0, 0],
        )
    })
    .map(|_| ())
}

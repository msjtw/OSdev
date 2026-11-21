use crate::{
    Result, abi,
    error::{decode, encode},
    syscall::syscall,
};

use super::{Event, Message, Request};

/// Revoke an owned submission and discard any unconsumed completion.
/// Success guarantees no further payload access/copy for this ID; bytes already
/// copied or transmitted are not rolled back. Errors are not confirmation.
pub fn cancel(id: usize) -> Result<()> {
    decode(unsafe { syscall(abi::SYS_IPC_CANCEL, [id, 0, 0, 0, 0, 0]) }).map(|_| ())
}

/// Submit without waiting for a reply.
///
/// # Safety
/// Any buffer referenced by the message must remain mapped and valid until this
/// request's completion is consumed or cancellation is confirmed. Reply buffers
/// must be writable and exclusively available for the kernel's copy; source
/// buffers must not be modified while the receiver may read them. An interrupted
/// event receive does not cancel submissions or end these obligations.
/// Concurrent submissions must not overlap writable reply storage or otherwise
/// violate these source/destination obligations.
/// The message structure itself is copied before this function returns.
pub unsafe fn submit(destination: usize, message: &Message) -> Result<usize> {
    decode(unsafe {
        syscall(
            abi::SYS_IPC_SUBMIT,
            [destination, message as *const Message as usize, 0, 0, 0, 0],
        )
    })
}

/// Receive a request or completion without allocating.
///
/// EINTR is an error receiving an event, not a completion of another operation.
/// No requests are automatically retried or cancelled.
pub fn receive_event() -> Result<Event> {
    let mut event = abi::IpcEvent::default();
    decode(unsafe {
        syscall(
            abi::SYS_IPC_RECEIVE_EVENT,
            [&mut event as *mut abi::IpcEvent as usize, 0, 0, 0, 0, 0],
        )
    })?;
    event.try_into()
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

pub const CUSTOM_BIT: usize = 1usize << (usize::BITS - 1);

pub const SYS_IPC_CALL: usize = CUSTOM_BIT | 1;
pub const SYS_IPC_RECEIVE: usize = CUSTOM_BIT | 2;
pub const SYS_IPC_READ_PAYLOAD: usize = CUSTOM_BIT | 3;
pub const SYS_IPC_REPLY: usize = CUSTOM_BIT | 4;

pub const SYS_READ: usize = 63;
pub const SYS_WRITE: usize = 64;
pub const SYS_EXIT: usize = 93;
pub const SYS_GETPID: usize = 172;

pub const KERNEL_SENDER_ID: usize = 0;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Message {
    pub sender: usize,
    pub operation: usize,
    pub args: [usize; 6],
}

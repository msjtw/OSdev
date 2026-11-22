pub const CUSTOM_BIT: usize = 1usize << (usize::BITS - 1);

pub const SYS_IPC_CALL: usize = CUSTOM_BIT | 1;
pub const SYS_IPC_RECEIVE: usize = CUSTOM_BIT | 2;
pub const SYS_IPC_READ_PAYLOAD: usize = CUSTOM_BIT | 3;
pub const SYS_IPC_REPLY: usize = CUSTOM_BIT | 4;
pub const SYS_IPC_SUBMIT: usize = CUSTOM_BIT | 5;
pub const SYS_IPC_RECEIVE_EVENT: usize = CUSTOM_BIT | 6;
pub const SYS_IPC_CANCEL: usize = CUSTOM_BIT | 7;

pub const SYS_TTY_CALLER_SESSION: usize = CUSTOM_BIT | 0x100;
pub const SYS_TTY_VALIDATE_FOREGROUND: usize = CUSTOM_BIT | 0x101;
pub const SYS_TTY_SIGNAL_GROUP: usize = CUSTOM_BIT | 0x102;
pub const SYS_TTY_PUBLISH_READINESS: usize = CUSTOM_BIT | 0x103;

pub const IPC_EVENT_REQUEST: usize = 0;
pub const IPC_EVENT_COMPLETION: usize = 1;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IpcEvent {
    pub kind: usize,
    pub id: usize,
    pub result: usize,
    pub message: Message,
}

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

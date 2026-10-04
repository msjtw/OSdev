use alloc::{sync::Arc, vec::Vec};

use crate::{
    lock::IntMutex,
    process::Process,
    uart::UART_TERMINAL,
};

#[derive(Clone, Copy, Debug)]
pub enum Errno {
    BadFileDescriptor,
    Fault,
    InvalidArgument,
    NotATerminal,
}

impl Errno {
    pub const fn as_retval(self) -> usize {
        let code = match self {
            Self::BadFileDescriptor => 9,
            Self::Fault => 14,
            Self::InvalidArgument => 22,
            Self::NotATerminal => 25,
        };
        (-code as isize) as usize
    }
}

pub trait FileOps: core::fmt::Debug + Send + Sync {
    fn read(&self, proc: &mut Process, addr: usize, len: usize) -> Result<usize, Errno>;
    fn write(&self, proc: &mut Process, addr: usize, len: usize) -> Result<usize, Errno>;
    fn ioctl(&self, proc: &mut Process, op: usize, arg: usize) -> Result<usize, Errno>;
}

#[derive(Clone, Copy, Debug)]
pub struct FileDescriptor {
    pub ops: &'static dyn FileOps,
    pub readable: bool,
    pub writable: bool,
}

impl FileDescriptor {
    pub const fn new(ops: &'static dyn FileOps, readable: bool, writable: bool) -> Self {
        Self {
            ops,
            readable,
            writable,
        }
    }
}

pub type FdTable = Vec<Option<FileDescriptor>>;
pub type SharedFdTable = Arc<IntMutex<FdTable>>;

pub fn standard_fds() -> FdTable {
    alloc::vec![
        Some(FileDescriptor::new(&UART_TERMINAL, true, false)),
        Some(FileDescriptor::new(&UART_TERMINAL, false, true)),
        Some(FileDescriptor::new(&UART_TERMINAL, false, true)),
    ]
}

pub fn new_standard_fds() -> SharedFdTable {
    Arc::new(IntMutex::new(standard_fds()))
}

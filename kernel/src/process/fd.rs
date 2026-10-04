use alloc::{sync::Arc, vec::Vec};

use crate::{lock::IntMutex, process::Process, uart::UART_TERMINAL};

#[derive(Clone, Copy, Debug)]
pub enum Errno {
    BadFileDescriptor,
    Fault,
    InvalidArgument,
    NotATerminal,
    BrokenPipe,
    TooManyFiles,
}

impl Errno {
    pub const fn as_retval(self) -> usize {
        let code = match self {
            Self::BadFileDescriptor => 9,
            Self::Fault => 14,
            Self::InvalidArgument => 22,
            Self::TooManyFiles => 24,
            Self::NotATerminal => 25,
            Self::BrokenPipe => 32,
        };
        (-code as isize) as usize
    }
}

pub trait FileOps: core::fmt::Debug + Send + Sync {
    fn read(&self, proc: &mut Process, addr: usize, len: usize) -> Result<usize, Errno>;
    fn write(&self, proc: &mut Process, addr: usize, len: usize) -> Result<usize, Errno>;
    fn ioctl(&self, proc: &mut Process, op: usize, arg: usize) -> Result<usize, Errno>;
}

#[derive(Debug)]
pub struct FileDescription {
    pub target: FileTarget,
    pub readable: bool,
    pub writable: bool,
}

impl FileDescription {
    pub fn new(target: FileTarget, readable: bool, writable: bool) -> Self {
        Self {
            target,
            readable,
            writable,
        }
    }
}

#[derive(Clone, Debug)]
pub enum FileTarget {
    Static(&'static dyn FileOps),
    Shared(Arc<dyn FileOps>),
}

impl FileTarget {
    pub fn ops(&self) -> &dyn FileOps {
        match self {
            Self::Static(ops) => *ops,
            Self::Shared(ops) => ops.as_ref(),
        }
    }
}

pub type FdTable = Vec<Option<Arc<FileDescription>>>;
pub type SharedFdTable = Arc<IntMutex<FdTable>>;
pub const MAX_FDS: usize = 64;

pub fn standard_fds() -> FdTable {
    alloc::vec![
        Some(Arc::new(FileDescription::new(
            FileTarget::Static(&UART_TERMINAL),
            true,
            false
        ))),
        Some(Arc::new(FileDescription::new(
            FileTarget::Static(&UART_TERMINAL),
            false,
            true
        ))),
        Some(Arc::new(FileDescription::new(
            FileTarget::Static(&UART_TERMINAL),
            false,
            true
        ))),
    ]
}

pub fn new_standard_fds() -> SharedFdTable {
    Arc::new(IntMutex::new(standard_fds()))
}

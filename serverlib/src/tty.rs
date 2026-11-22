//! Terminal IPC uses [fd, buffer pointer, buffer length, ioctl number, 0, 0].
//! Shared ABI, serialization and syscall wrappers only; terminal policy belongs to tty_driver.
use crate::{Error, Result, abi::CUSTOM_BIT};
#[cfg(all(
    feature = "userspace",
    any(target_arch = "riscv32", target_arch = "riscv64")
))]
mod mechanisms;
#[cfg(all(
    feature = "userspace",
    any(target_arch = "riscv32", target_arch = "riscv64")
))]
pub use mechanisms::*;

pub const IOCTL: usize = CUSTOM_BIT | 0x200;
pub const TCGETS: usize = 0x5401;
pub const TCSETS: usize = 0x5402;
pub const TIOCGPGRP: usize = 0x540f;
pub const TIOCSPGRP: usize = 0x5410;
pub const TIOCGWINSZ: usize = 0x5413;
pub const ICRNL: u32 = 0o000400;
pub const ECHO: u32 = 0o000010;
pub const ICANON: u32 = 0o000002;
pub const ISIG: u32 = 0o000001;
pub const VINTR: usize = 0;
pub type PollEvents = i16;
pub const POLLIN: PollEvents = 0x001;
pub const POLLOUT: PollEvents = 0x004;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Termios {
    pub c_iflag: u32,
    pub c_oflag: u32,
    pub c_cflag: u32,
    pub c_lflag: u32,
    pub c_line: u8,
    pub c_cc: [u8; 19],
}

impl Termios {
    pub const SIZE: usize = 36;

    /// Encode the native Linux termios layout without exposing padding.
    pub fn to_bytes(self) -> [u8; Self::SIZE] {
        let mut bytes = [0; Self::SIZE];
        for (index, value) in [self.c_iflag, self.c_oflag, self.c_cflag, self.c_lflag]
            .iter()
            .enumerate()
        {
            bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_ne_bytes());
        }
        bytes[16] = self.c_line;
        bytes[17..].copy_from_slice(&self.c_cc);
        bytes
    }

    /// Decode a complete native termios payload while preserving unsupported fields.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != Self::SIZE {
            return Err(Error::EINVAL);
        }
        let word = |offset| u32::from_ne_bytes(bytes[offset..offset + 4].try_into().unwrap());
        let mut c_cc = [0; 19];
        c_cc.copy_from_slice(&bytes[17..]);
        Ok(Self {
            c_iflag: word(0),
            c_oflag: word(4),
            c_cflag: word(8),
            c_lflag: word(12),
            c_line: bytes[16],
            c_cc,
        })
    }
}

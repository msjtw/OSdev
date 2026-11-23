//! Raw block IPC contract; no device, cache, filesystem or DMA policy.
use crate::{Error, Result, abi::{CUSTOM_BIT, Message}};

pub const INFO: usize = CUSTOM_BIT | 0x400;
pub const READ_BLOCK: usize = CUSTOM_BIT | 0x401;
pub const WRITE_BLOCK: usize = CUSTOM_BIT | 0x402;
pub const FLUSH: usize = CUSTOM_BIT | 0x403;
/// Kernel-only progress notification; args[0] is a coalesced flush-due boolean.
pub const POLL: usize = CUSTOM_BIT | 0x404;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Geometry {
    pub block_size: u32,
    pub block_count: u64,
}

impl Geometry {
    pub const SIZE: usize = 16;

    pub fn to_bytes(self) -> [u8; Self::SIZE] {
        let mut bytes = [0; Self::SIZE];
        bytes[..4].copy_from_slice(&self.block_size.to_le_bytes());
        bytes[8..].copy_from_slice(&self.block_count.to_le_bytes());
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != Self::SIZE || bytes[4..8] != [0; 4] {
            return Err(Error::EINVAL);
        }
        Ok(Self {
            block_size: u32::from_le_bytes(bytes[..4].try_into().unwrap()),
            block_count: u64::from_le_bytes(bytes[8..].try_into().unwrap()),
        })
    }
}

pub const fn index_words(index: u64) -> (usize, usize) {
    (index as u32 as usize, (index >> 32) as u32 as usize)
}

pub fn block_index(low: usize, high: usize) -> Result<u64> {
    let low = u32::try_from(low).map_err(|_| Error::EINVAL)?;
    let high = u32::try_from(high).map_err(|_| Error::EINVAL)?;
    Ok(u64::from(low) | (u64::from(high) << 32))
}

/// Validate wire shape only. Authorization and device geometry are server checks.
pub fn validate(message: &Message) -> Result<()> {
    let a = message.args;
    let valid = match message.operation {
        INFO => a[0] == 0 && a[2] == Geometry::SIZE && a[3..] == [0; 3],
        READ_BLOCK | WRITE_BLOCK => {
            block_index(a[0], a[3])?;
            a[2] != 0 && a[4..] == [0; 2]
        }
        FLUSH => a == [0; 6],
        POLL => a[0] <= 1 && a[1..] == [0; 5],
        _ => return Err(Error::ENOSYS),
    };
    if valid { Ok(()) } else { Err(Error::EINVAL) }
}

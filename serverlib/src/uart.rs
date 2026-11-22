//! UART service operations, distinct from kernel syscall numbers.
use crate::abi::CUSTOM_BIT;

/// Kernel-only, payload-free request to progress deferred UART work.
pub const POLL: usize = CUSTOM_BIT | 0x300;

#![no_std]

pub mod abi;
pub mod error;

pub use error::{Error, Result};

#[cfg(all(
    feature = "userspace",
    any(target_arch = "riscv32", target_arch = "riscv64")
))]
pub mod ipc;

#[cfg(all(
    feature = "userspace",
    any(target_arch = "riscv32", target_arch = "riscv64")
))]
pub mod syscall;

#![no_std]
#![cfg_attr(
    all(
        feature = "userspace",
        any(target_arch = "riscv32", target_arch = "riscv64")
    ),
    feature(alloc_error_handler)
)]

pub mod abi;
#[cfg(feature = "userspace")]
pub mod allocator;
pub mod error;
pub mod runtime;

pub use error::{Error, Result};
pub use runtime::Args;

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

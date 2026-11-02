use core::ffi::{CStr, c_char};

use crate::{Error, Result};

pub const STACK_ALIGNMENT: usize = 16;
pub const HEAP_ALIGNMENT: usize = 4096;
pub const EXIT_FAILURE: i32 = 1;
pub const EXIT_USAGE: i32 = 2;

pub struct Args {
    argv: *const *const c_char,
    argc: usize,
}

impl Args {
    pub unsafe fn from_stack(stack: *const usize) -> Self {
        Self {
            argc: unsafe { stack.read() },
            argv: unsafe { stack.add(1) }.cast(),
        }
    }

    pub fn len(&self) -> usize {
        self.argc
    }

    pub fn is_empty(&self) -> bool {
        self.argc == 0
    }

    pub fn get(&self, index: usize) -> Option<&CStr> {
        if index >= self.argc {
            return None;
        }
        Some(unsafe { CStr::from_ptr(self.argv.add(index).read()) })
    }

    pub fn parse_usize(&self, index: usize) -> Result<usize> {
        let bytes = self.get(index).ok_or(Error::EINVAL)?.to_bytes();
        let (digits, radix) = match bytes
            .strip_prefix(b"0x")
            .or_else(|| bytes.strip_prefix(b"0X"))
        {
            Some(hex) => (hex, 16usize),
            None => (bytes, 10usize),
        };
        if digits.is_empty() {
            return Err(Error::EINVAL);
        }
        let mut value = Some(0usize);
        for &byte in digits {
            let digit = match byte {
                b'0'..=b'9' => (byte - b'0') as usize,
                b'a'..=b'f' => (byte - b'a') as usize + 10,
                b'A'..=b'F' => (byte - b'A') as usize + 10,
                _ => return Err(Error::EINVAL),
            };
            if digit >= radix {
                return Err(Error::EINVAL);
            }
            value = value.and_then(|v| v.checked_mul(radix)?.checked_add(digit));
        }
        value.ok_or(Error::EOVERFLOW)
    }
}

#[cfg(all(
    feature = "userspace",
    any(target_arch = "riscv32", target_arch = "riscv64")
))]
#[macro_export]
macro_rules! entry {
    ($main:path) => {
        $crate::entry!($main, panic = $crate::runtime::default_panic);
    };
    ($main:path, panic = $panic:path) => {
        #[global_allocator]
        static __SERVER_HEAP: $crate::allocator::FixedHeap = $crate::allocator::FixedHeap::new();

        #[unsafe(no_mangle)]
        #[unsafe(naked)]
        #[unsafe(link_section = ".text.entry")]
        pub unsafe extern "C" fn _start() -> ! {
            core::arch::naked_asm!(
                ".option push",
                ".option norelax",
                "la gp, __global_pointer$",
                ".option pop",
                "mv a0, sp",
                "tail {entry}",
                entry = sym __server_entry,
            );
        }

        unsafe extern "C" fn __server_entry(stack: *const usize) -> ! {
            let main: fn($crate::Args) -> ! = $main;
            let args = unsafe { $crate::Args::from_stack(stack) };
            unsafe extern "C" {
                static __server_heap_start: u8;
                static __server_heap_end: u8;
            }
            let start = core::ptr::addr_of!(__server_heap_start) as usize;
            let end = core::ptr::addr_of!(__server_heap_end) as usize;
            let size = match end.checked_sub(start) {
                Some(size) => size,
                None => $crate::syscall::exit($crate::runtime::EXIT_FAILURE),
            };
            if unsafe { __SERVER_HEAP.init(start, size) }.is_err() {
                $crate::syscall::exit($crate::runtime::EXIT_FAILURE);
            }
            main(args)
        }

        #[panic_handler]
        fn __server_panic(info: &core::panic::PanicInfo<'_>) -> ! {
            let handler: fn(&core::panic::PanicInfo<'_>) -> ! = $panic;
            handler(info)
        }
    };
}

#[cfg(all(
    feature = "userspace",
    any(target_arch = "riscv32", target_arch = "riscv64")
))]
pub fn default_panic(_: &core::panic::PanicInfo<'_>) -> ! {
    crate::syscall::exit(EXIT_FAILURE)
}

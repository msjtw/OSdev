// ansi color guide:
// https://gist.github.com/JBlond/2fea43a3049b38287e5e9cefc87b2124

use core::sync::atomic::{AtomicU8, Ordering};

pub const TRACE: u8 = 1 << 0;
pub const DEBUG: u8 = 1 << 1;
pub const INFO: u8 = 1 << 2;
pub const WARN: u8 = 1 << 3;
pub const ERROR: u8 = 1 << 4;

static LOG_MASK: AtomicU8 = AtomicU8::new(0);

pub fn enabled(level: u8) -> bool {
    LOG_MASK.load(Ordering::Relaxed) & level != 0
}

pub fn set_mask(mask: u8) {
    LOG_MASK.store(mask, Ordering::Relaxed);
}

pub fn enable(mask: u8) {
    LOG_MASK.fetch_or(mask, Ordering::Relaxed);
}

pub fn disable(mask: u8) {
    LOG_MASK.fetch_and(!mask, Ordering::Relaxed);
}

#[macro_export]
macro_rules! trace {
    ($($arg:tt)*) => {{
        if $crate::log::enabled($crate::log::TRACE) {
            $crate::logln!("\x1b[90m[TRACE]\x1b[0m {}", core::format_args!($($arg)*));
        }
    }};
}
pub use crate::trace;

#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {{
        if $crate::log::enabled($crate::log::DEBUG) {
            $crate::logln!("\x1b[96m[DEBUG]\x1b[0m {}", core::format_args!($($arg)*));
        }
    }};
}
pub use crate::debug;

#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {{
        if $crate::log::enabled($crate::log::INFO) {
            $crate::logln!("\x1b[92m[INFO] \x1b[0m {}", core::format_args!($($arg)*));
        }
    }};
}
pub use crate::info;

#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {{
        if $crate::log::enabled($crate::log::WARN) {
            $crate::logln!("\x1b[93m[WARN] \x1b[0m {}", core::format_args!($($arg)*));
        }
    }};
}
pub use crate::warn;

#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {{
        if $crate::log::enabled($crate::log::ERROR) {
            $crate::logln!("\x1b[91m[ERROR]\x1b[0m {}", core::format_args!($($arg)*));
        }
    }};
}
pub use crate::error;

#[macro_export]
macro_rules! logln {
    () => {
        $crate::log!("\n")
    };
    ($($arg:tt)*) => {{
        $crate::log!($($arg)*);
        $crate::log!("\n")
    }};
}
pub use crate::logln;

#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => {
        {
            use core::fmt::Write;
            let mut formatter = $crate::drivers::uart::StackFormatter::new();
            let _ = core::write!(formatter, $($arg)*);
            formatter.flush();
        }
    };
}

// Compatibility names used by imported drivers.  Keep their implementation on
// the same heap-free UART path as the logging macros above.
#[macro_export]
macro_rules! uart_print {
    ($($arg:tt)*) => {
        $crate::log!($($arg)*)
    };
}
pub use crate::uart_print;

#[macro_export]
macro_rules! uart_println {
    () => {
        $crate::logln!()
    };
    ($($arg:tt)*) => {
        $crate::logln!($($arg)*)
    };
}
pub use crate::uart_println;

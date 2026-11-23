use core::fmt::Arguments;
use spin::Once;

#[derive(Clone, Copy)]
pub enum Level { Trace, Debug, Info, Error }
type Logger = fn(Level, Arguments<'_>);
static LOGGER: Once<Logger> = Once::new();

/// Install an optional allocation-free diagnostic sink before initializing devices.
pub fn set_logger(logger: Logger) {
    LOGGER.call_once(|| logger);
}

pub(crate) fn emit(level: Level, args: Arguments<'_>) {
    if let Some(logger) = LOGGER.get() { logger(level, args); }
}

macro_rules! trace { ($($arg:tt)*) => { $crate::log::emit($crate::Level::Trace, core::format_args!($($arg)*)) }; }
macro_rules! debug { ($($arg:tt)*) => { $crate::log::emit($crate::Level::Debug, core::format_args!($($arg)*)) }; }
macro_rules! info { ($($arg:tt)*) => { $crate::log::emit($crate::Level::Info, core::format_args!($($arg)*)) }; }
macro_rules! error { ($($arg:tt)*) => { $crate::log::emit($crate::Level::Error, core::format_args!($($arg)*)) }; }
pub(crate) use {trace, debug, info, error};
pub(crate) use info as uart_println;

#![no_std]

//! No-std logging trait for PDF utilities
//! 
//! This trait allows the library to output debug information without
//! depending on any specific hardware or zkVM implementation.

extern crate alloc;

use alloc::format;
use core::fmt;

/// A trait for logging debug messages in a no_std environment
pub trait Logger {
    /// Log a debug message
    fn log_debug(&self, message: &str);
    
    /// Log a formatted debug message
    fn log_debug_fmt(&self, args: fmt::Arguments<'_>) {
        let message = format!("{}", args);
        self.log_debug(&message);
    }
}

/// A no-op logger that discards all messages
pub struct NullLogger;

impl Logger for NullLogger {
    fn log_debug(&self, _message: &str) {
        // Do nothing
    }
}

/// Global logger instance - should be set by the binary
static mut LOGGER: Option<&'static dyn Logger> = None;

/// Set the global logger
/// 
/// # Safety
/// This function should only be called once at program startup
pub unsafe fn set_logger(logger: &'static dyn Logger) {
    LOGGER = Some(logger);
}

/// Log a debug message using the global logger
pub fn log_debug(message: &str) {
    unsafe {
        if let Some(logger) = LOGGER {
            logger.log_debug(message);
        }
    }
}

/// Macro for logging debug messages
#[macro_export]
macro_rules! debug_log {
    ($($arg:tt)*) => {{
        #[cfg(feature = "debug")]
        {
            use $crate::log_debug;
            log_debug(&alloc::format!($($arg)*));
        }
    }};
}
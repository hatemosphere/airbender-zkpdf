//! UART logger implementation for zkVM environment

use core::fmt::Write;
use pdf_utils_zkvm_core::Logger;
use riscv_common::QuasiUART;

/// A logger that outputs to UART in the zkVM
pub struct UartLogger;

impl Logger for UartLogger {
    fn log_debug(&self, message: &str) {
        let mut uart = QuasiUART::new();
        let _ = write!(uart, "{}", message);
    }

    fn log_debug_fmt(&self, args: core::fmt::Arguments<'_>) {
        let mut uart = QuasiUART::new();
        let _ = write!(uart, "{}", args);
    }
}

/// Global instance of the UART logger
pub static UART_LOGGER: UartLogger = UartLogger;

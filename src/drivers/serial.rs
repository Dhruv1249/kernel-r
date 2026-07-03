// src/drivers/serial.rs

use core::fmt;
use lazy_static::lazy_static;
use uart_16550::SerialPort;

lazy_static! {
    /// The global, lazily-initialised UART 16550A serial port singleton on COM1 (`0x3F8`).
    ///
    /// Initialised on first use: the `uart_16550` crate configures the UART baud
    /// rate, parity, stop bits, and FIFOs.  Wrapped in
    /// [`crate::mm::allocator::Locked`] so it can be safely written from both
    /// normal kernel code and interrupt handlers (e.g. the keyboard ISR).
    ///
    /// Serial output is the primary debugging channel — it is visible in QEMU's
    /// `-serial stdio` output and in the `qemu.log` file.
    // Basically we have no heap atm so we need to initialize this lazy
    // i.e at runtime not at compile time
    pub static ref SERIAL1: crate::mm::allocator::Locked<SerialPort> = crate::mm::allocator::Locked::new({
        let mut serial_port = unsafe { SerialPort::new(0x3F8) };
        serial_port.init();
        serial_port
    });
}
// Mostly copied from the official print macro defininition
#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => ($crate::drivers::serial::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! serial_println {
    () => ($crate::serial_print!("\n"));
    ($($arg:tt)*) => ($crate::serial_print!("{}\n", format_args!($($arg)*)));
}

/// Internal print helper called by both the `serial_print!` and `serial_println!` macros.
///
/// Acquires the global [`SERIAL1`] lock and writes the formatted arguments to
/// the UART.  Uses `let _ =` to suppress the `Result` so a serial write failure
/// does not cause a panic inside the panic handler.
#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    // Revoed unwrap to prevent panic within panic
    let _ = SERIAL1.lock().write_fmt(args);
}

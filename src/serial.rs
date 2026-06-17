use lazy_static::lazy_static;
use uart_16550::SerialPort;
use core::fmt; 


// Basically we have no heap atm so we need to initialize this lazy
// i.e at runtime not at compile time
lazy_static! {
    pub static ref SERIAL1: crate::allocator::Locked<SerialPort> = crate::allocator::Locked::new({
        let mut serial_port = unsafe { SerialPort::new(0x3F8) };
        serial_port.init();
        serial_port
    });
}
// Mostly copied from the official print macro defininition
#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => ($crate::serial::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! serial_println {
    () => ($crate::serial_print!("\n"));
    ($($arg:tt)*) => ($crate::serial_print!("{}\n", format_args!($($arg)*)));
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    // Revoed unwrap to prevent panic within panic
    let _ = SERIAL1.lock().write_fmt(args);
}

//! # Hardware Drivers
//!
//! Thin, kernel-space device drivers for the hardware components used by this
//! kernel:
//!
//! | Module | Role |
//! |--------|------|
//! | [`vga_buffer`] | VGA text-mode framebuffer writer + `print!`/`println!` macros |
//! | [`serial`] | UART 16550A serial port driver + `serial_print!`/`serial_println!` macros |
//! | [`keyboard`] | PS/2 keyboard state machine using `pc-keyboard`; exposes `KEYBOARD_MAILBOX` |
//! | [`qemu`] | QEMU-specific ISA debug-exit port for controlled test exits |

pub mod vga_buffer;
pub mod serial;
pub mod keyboard;
pub mod qemu;

// src/drivers/vga_buffer.rs

use core::fmt;
use lazy_static::lazy_static;
use volatile::Volatile;

/// The 16 standard VGA text-mode colours, encoded as 4-bit values.
///
/// The value of each variant matches the VGA hardware colour-code directly:
/// bits 0–3 of the attribute byte select the foreground colour and bits 4–7
/// select the background colour (values 0–7 only for background, with bit 3
/// as the "bright" flag on some implementations).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Color {
    Black = 0,
    Blue = 1,
    Green = 2,
    Cyan = 3,
    Red = 4,
    Magenta = 5,
    Brown = 6,
    LightGray = 7,
    DarkGray = 8,
    LightBlue = 9,
    LightGreen = 10,
    LightCyan = 11,
    LightRed = 12,
    Pink = 13,
    Yellow = 14,
    White = 15,
}

/// A packed pair of foreground and background colour codes.
///
/// Stored as a single byte: bits 4–7 = background, bits 0–3 = foreground.
/// `#[repr(transparent)]` ensures the struct has the same memory layout as
/// the inner `u8`, enabling safe casts in the VGA buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
struct ColorCode(u8);

impl ColorCode {
    /// Creates a `ColorCode` from a foreground and background [`Color`].
    fn new(foreground: Color, background: Color) -> ColorCode {
        ColorCode((background as u8) << 4 | (foreground as u8))
    }
}

/// A single character cell in the VGA text-mode buffer.
///
/// Each cell is 2 bytes: one ASCII character byte and one colour-code byte.
/// `#[repr(C)]` ensures the two fields are laid out in the order expected by
/// VGA hardware (character byte first, colour byte second).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
struct ScreenChar {
    ascii_character: u8,
    color_code: ColorCode,
}

/// Standard VGA text-mode screen dimensions.
const BUFFER_HEIGHT: usize = 25;
/// Standard VGA text-mode screen width.
const BUFFER_WIDTH: usize = 80;

/// The raw VGA text-mode framebuffer, mapped at physical address `0xB8000`.
///
/// The `Volatile` wrapper prevents the compiler from optimising away writes
/// to the buffer, which it might otherwise do because the data is never read
/// back by Rust code (only by the VGA hardware).
#[repr(transparent)]
struct Buffer {
    // Building 2d array for VGA buffer
    chars: [[Volatile<ScreenChar>; BUFFER_WIDTH]; BUFFER_HEIGHT],
}

/// The kernel VGA text-mode writer.
///
/// Maintains a cursor at `column_position` in the bottom row and scrolls the
/// screen upward by one row when a newline is encountered or the line is full.
/// All writes go through `Volatile` cells to guarantee they reach hardware.
pub struct Writer {
    column_position: usize,
    color_code: ColorCode,
    buffer: &'static mut Buffer,
}

impl Writer {
    /// Writes a single ASCII byte to the VGA buffer.
    ///
    /// Newlines (`\n`) trigger [`Writer::new_line`].  All other bytes are
    /// written into the bottom row at `column_position` and the cursor is
    /// advanced.  If the cursor reaches the right edge, a newline is forced
    /// before writing the byte.
    pub fn write_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => self.new_line(),
            byte => {
                if self.column_position >= BUFFER_WIDTH {
                    self.new_line();
                }
                let row = BUFFER_HEIGHT - 1;
                let col = self.column_position;

                let color = self.color_code;
                self.buffer.chars[row][col].write(ScreenChar {
                    ascii_character: byte,
                    color_code: color,
                });

                self.column_position += 1;
            }
        }
    }

    /// Scrolls the screen up by one row and resets the cursor to column 0.
    ///
    /// Copies each row's contents one row upward (row `i` → row `i-1`), then
    /// clears the last row.  Called automatically when a newline is written or
    /// the column cursor overflows the screen width.
    fn new_line(&mut self) {
        for row in 1..BUFFER_HEIGHT {
            for col in 0..BUFFER_WIDTH {
                let character = self.buffer.chars[row][col].read();
                self.buffer.chars[row - 1][col].write(character);
            }
        }

        self.clear_row(BUFFER_HEIGHT - 1);
        self.column_position = 0;
    }

    /// Fills the given `row` with blank space characters in the current colour.
    fn clear_row(&mut self, row: usize) {
        let blank_char = ScreenChar {
            ascii_character: b' ',
            color_code: self.color_code,
        };

        for col in 0..BUFFER_WIDTH {
            self.buffer.chars[row][col].write(blank_char);
        }
    }

    /// Clears the entire VGA screen by blanking all rows.
    pub fn clear(&mut self) {
        for row in 0..BUFFER_HEIGHT {
            self.clear_row(row);
        }
    }

    /// Writes a UTF-8 string to the VGA buffer, replacing non-ASCII bytes with `■` (`0xFE`).
    ///
    /// Only bytes in the printable ASCII range `0x20–0x7E` and newlines are
    /// written verbatim.  Everything else (e.g., multi-byte UTF-8 continuation
    /// bytes) is replaced with the VGA `■` glyph (`0xFE`) so the screen
    /// always shows something meaningful.
    pub fn write_string(&mut self, s: &str) {
        for byte in s.bytes() {
            match byte {
                b'\n' => self.new_line(),
                // Print only if the byte is in ascii range
                // i.e 32 to 126
                0x20..=0x7e => self.write_byte(byte),
                // Prints ■ if out of ascii range
                _ => self.write_byte(0xfe),
            }
        }
    }
}

impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_string(s);
        Ok(())
    }
}


lazy_static! {
    /// The global, lazily-initialised VGA [`Writer`] singleton.
    ///
    /// Wrapped in [`crate::mm::allocator::Locked`] so it can be safely shared
    /// between normal kernel code and interrupt handlers.  Initialised on first
    /// use via `lazy_static!` because the VGA buffer address requires
    /// `paging::PHYS_OFFSET` which is a runtime constant.
    // Basically we have no heap atm so we need to initialize this lazy
    // i.e at runtime not at compile time
    pub static ref WRITER: crate::mm::allocator::Locked<Writer> = crate::mm::allocator::Locked::new(Writer {
        column_position: 0,
        color_code: ColorCode::new(Color::Yellow, Color::Black),
        buffer: unsafe { &mut *(( 0xb8000 + crate::mm::paging::PHYS_OFFSET) as *mut Buffer) },
    });
}

// Defining our prin macros
// Mostly copied from the official print macro defininition
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::drivers::vga_buffer::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

/// Internal print helper called by both the `print!` and `println!` macros.
///
/// Acquires the global [`WRITER`] lock and delegates to
/// [`core::fmt::Write::write_fmt`].  Uses `let _ =` instead of `.unwrap()`
/// to prevent a panic inside a panic handler (which would cause a double
/// panic and hang the system).
#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    // Revoed unwrap to prevent panic within panic
    let _ = WRITER.lock().write_fmt(args);
}

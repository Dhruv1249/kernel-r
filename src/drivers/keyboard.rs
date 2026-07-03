// src/drivers/keyboard.rs

use pc_keyboard::{HandleControl, Keyboard, ScancodeSet1, layouts};

/// Global PS/2 keyboard state machine.
///
/// Uses the `pc-keyboard` crate's `Keyboard` type configured for the
/// US 104-key layout and PS/2 Scancode Set 1 (the scancode set used by BIOS
/// and most PC emulators including QEMU).
///
/// On every keyboard interrupt, `interrupt::keyboard_handler` reads a raw
/// scancode from I/O port `0x60` and feeds it to this state machine with
/// `keyboard.add_byte(scancode)`.  When the state machine accumulates enough
/// bytes to recognise a key event, it returns a `DecodedKey`.
// We use the standard US 104-key layout and PS/2 Scancode Set 1
pub static KEYBOARD: spin::Mutex<Keyboard<layouts::Us104Key, ScancodeSet1>> =
    spin::Mutex::new(Keyboard::new(
        ScancodeSet1::new(),
        layouts::Us104Key,
        HandleControl::Ignore,
    ));

lazy_static::lazy_static! {
    /// Mailbox used to pass decoded key events from the keyboard ISR to kernel tasks.
    /// The keyboard ISR sends a [`pc_keyboard::DecodedKey`] here after the
    /// `pc-keyboard` state machine produces a complete key event.  Kernel threads
    /// (or user-space proxies) call `receive()` to block until a key is pressed.
    pub static ref KEYBOARD_MAILBOX: crate::sync::ipc::Mailbox<pc_keyboard::DecodedKey> = crate::sync::ipc::Mailbox::new();
}

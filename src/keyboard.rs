// src/keyboard.rs

use pc_keyboard::{layouts, HandleControl, Keyboard, ScancodeSet1};

// We use the standard US 104-key layout and PS/2 Scancode Set 1
pub static KEYBOARD: spin::Mutex<Keyboard<layouts::Us104Key, ScancodeSet1>> = spin::Mutex::new(Keyboard::new(
    ScancodeSet1::new(),
    layouts::Us104Key,
    HandleControl::Ignore,
));


lazy_static::lazy_static! {
    pub static ref KEYBOARD_MAILBOX: crate::ipc::Mailbox<pc_keyboard::DecodedKey> = crate::ipc::Mailbox::new();
}

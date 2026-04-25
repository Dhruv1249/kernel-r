// src/keyboard.rs

use pc_keyboard::{Keyboard, layouts, HandleControl, ScancodeSet1};
use spin::Mutex;

// We use the standard US 104-key layout and PS/2 Scancode Set 1
pub static KEYBOARD: Mutex<Keyboard<layouts::Us104Key, ScancodeSet1>> = Mutex::new(
    Keyboard::new(ScancodeSet1::new(), layouts::Us104Key, HandleControl::Ignore)
);


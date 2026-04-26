// src/keyboard.rs

use pc_keyboard::{layouts, HandleControl, Keyboard, ScancodeSet1};
use spin::Mutex;

// We use the standard US 104-key layout and PS/2 Scancode Set 1
pub static KEYBOARD: Mutex<Keyboard<layouts::Us104Key, ScancodeSet1>> = Mutex::new(Keyboard::new(
    ScancodeSet1::new(),
    layouts::Us104Key,
    HandleControl::Ignore,
));

// SPSC ring buffer for keyboard events
pub static KEYBOARD_EVENTS: crate::queue::RingBuffer<pc_keyboard::DecodedKey, 1024> =
    crate::queue::RingBuffer::new();

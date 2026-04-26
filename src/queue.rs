// src/queue.rs

use core::sync::atomic::{AtomicUsize, Ordering};

// Single producer, single consumer (SPSC) ring buffer
pub struct RingBuffer<T, const N: usize> {
    buffer: core::cell::UnsafeCell< [Option<T>; N]>,
    head: AtomicUsize,
    tail: AtomicUsize,
}

impl<T: , const N: usize> RingBuffer<T, N> {
    const INIT: Option<T> = None;
    pub const fn new() -> Self {
        Self {
            buffer: core::cell::UnsafeCell::new([Self::INIT; N]),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    // Producer
    pub fn push(&self, item: T) -> Result<(), T> {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        let next_head = (head + 1) % N;

        if next_head == tail {
            return Err(item);
        }
       unsafe {  (*self.buffer.get())[head] = Some(item); }
        self.head.store(next_head, Ordering::Release);

        Ok(())
    }

    // Consumer
    pub fn pop(&self) -> Option<T> {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        let next_tail = (tail + 1) % N;

        if tail == head {
            return None;
        }
        let item = unsafe { (*self.buffer.get())[tail].take() };
        unsafe { (*self.buffer.get())[tail] = None; }
        self.tail.store(next_tail, Ordering::Release);

        item
    }
}

unsafe impl<T, const N: usize> Sync for RingBuffer<T, N> {}

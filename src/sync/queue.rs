// src/sync/queue.rs

use core::sync::atomic::{AtomicUsize, Ordering};

/// A lock-free, single-producer single-consumer (SPSC) ring buffer.
///
/// # Design
///
/// `RingBuffer<T, N>` uses two `AtomicUsize` indices — `head` (producer
/// cursor) and `tail` (consumer cursor) — to communicate between one writer
/// thread and one reader thread **without any locks**.
///
/// The buffer is considered full when `(head + 1) % N == tail` (one slot is
/// always sacrificed to distinguish full from empty).  It is empty when
/// `head == tail`.
///
/// # Ordering
/// - Producer: writes the slot first, then advances `head` with
///   `Ordering::Release` so the consumer sees the new data.
/// - Consumer: reads `head` with `Ordering::Acquire` before reading the slot,
///   ensuring the load sees the completed write.
///
/// # Safety
/// The `unsafe impl Sync` is sound only when exactly one thread calls `push`
/// and exactly one thread calls `pop`.  Using multiple producers or consumers
/// without external synchronisation is undefined behaviour.
// Single producer, single consumer (SPSC) ring buffer
pub struct RingBuffer<T, const N: usize> {
    buffer: core::cell::UnsafeCell<[Option<T>; N]>,
    head: AtomicUsize,
    tail: AtomicUsize,
}

impl<T, const N: usize> RingBuffer<T, N> {
    const INIT: Option<T> = None;

    /// Creates an empty `RingBuffer` with all slots initialised to `None`.
    pub const fn new() -> Self {
        Self {
            buffer: core::cell::UnsafeCell::new([Self::INIT; N]),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    /// Attempts to enqueue `item` at the head of the ring buffer.
    ///
    /// Returns `Ok(())` on success.  Returns `Err(item)` (giving the item
    /// back to the caller) if the buffer is full (`(head + 1) % N == tail`).
    // Producer
    pub fn push(&self, item: T) -> Result<(), T> {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        let next_head = (head + 1) % N;

        if next_head == tail {
            return Err(item);
        }
        unsafe {
            (*self.buffer.get())[head] = Some(item);
        }
        self.head.store(next_head, Ordering::Release);

        Ok(())
    }

    /// Attempts to dequeue and return the oldest item from the ring buffer.
    ///
    /// Returns `Some(item)` on success, or `None` if the buffer is empty
    /// (`head == tail`).  After taking the item, the slot is cleared to
    /// `None` and `tail` is advanced with `Ordering::Release`.
    // Consumer
    pub fn pop(&self) -> Option<T> {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        let next_tail = (tail + 1) % N;

        if tail == head {
            return None;
        }
        let item = unsafe { (*self.buffer.get())[tail].take() };
        unsafe {
            (*self.buffer.get())[tail] = None;
        }
        self.tail.store(next_tail, Ordering::Release);

        item
    }
}

unsafe impl<T, const N: usize> Sync for RingBuffer<T, N> {}

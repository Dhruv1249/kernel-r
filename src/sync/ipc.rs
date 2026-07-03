// src/sync/ipc.rs

/// A blocking message-passing mailbox for inter-task communication.
///
/// `Mailbox<T>` combines a [`crate::mm::allocator::Locked`]-protected
/// `VecDeque` with a [`crate::sync::sync::Semaphore`] to create a blocking
/// FIFO channel:
///
/// - **Sender** ([`Mailbox::send`]): pushes the message onto the queue, then
///   calls `semaphore.release()` to increment the available-item count and
///   wake any sleeping receiver.
/// - **Receiver** ([`Mailbox::receive`]): calls `semaphore.acquire()`, which
///   blocks until at least one message is available, then pops the front of
///   the queue.
///
/// The semaphore count always equals the number of messages in the queue,
/// providing a clean, deadlock-free synchronisation contract.
pub struct Mailbox<T> {
    // We use a Mutex to protect the inner queue from concurrent access.
    queue: crate::mm::allocator::Locked<alloc::collections::VecDeque<T>>,
    
    // We use a Semaphore to count available items and block tasks when empty.
    available: crate::sync::sync::Semaphore,
}

impl<T> Mailbox<T> {
    /// Creates a new, empty `Mailbox`.
    ///
    /// `const fn` so it can be used in `static` initialisers.
    pub const fn new() -> Self {
        // Initialization
        Self {
            queue: crate::mm::allocator::Locked::new(alloc::collections::VecDeque::new()),
            available: crate::sync::sync::Semaphore::new(0),
        }
    }

    /// Enqueues `message` and wakes one waiting receiver.
    ///
    /// Always succeeds immediately — the queue is unbounded.
    pub fn send(&self, message: T) {
        self.queue.lock().push_back(message);
        self.available.release();
    }

    /// Blocks until a message is available, then dequeues and returns it.
    ///
    /// The `acquire()` call puts the current thread to sleep if the queue
    /// is empty.  When woken by a `send`, the thread pops the front of the
    /// `VecDeque` under its lock and returns the message.
    pub fn receive(&self) -> Option<T> {
        self.available.acquire();
        self.queue.lock().pop_front()
    }
}

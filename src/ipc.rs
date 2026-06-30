// src/ipc.rs

pub struct Mailbox<T> {
    // We use a Mutex to protect the inner queue from concurrent access.
    queue: crate::sync::Mutex<alloc::collections::VecDeque<T>>,
    
    // We use a Semaphore to count available items and block tasks when empty.
    available: crate::sync::Semaphore,
}

impl<T> Mailbox<T> {
    pub const fn new() -> Self {
        // Initialization
        Self {
            queue: crate::sync::Mutex::new(alloc::collections::VecDeque::new()),
            available: crate::sync::Semaphore::new(0),
        }
    }

    pub fn send(&self, message: T) {
        self.queue.lock().push_back(message);
        self.available.release();
    }

    pub fn receive(&self) -> Option<T> {
        self.available.acquire();
        self.queue.lock().pop_front()
    }
}

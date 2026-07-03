// src/sync/sync.rs

/// The mutable state of a [`WaitQueue`], kept separate so it can be protected by a spinlock.
///
/// Stores the ordered deque of thread IDs (TIDs) currently sleeping on the queue.
pub struct QueueState {
    pub waiters: alloc::collections::VecDeque<usize>,
}

/// A blocking wait-queue that allows threads to sleep until a condition is met.
///
/// Threads add their TID to `state.waiters` and then yield the CPU.  A waker
/// (e.g., a mutex unlock or semaphore release) calls [`WaitQueue::wake_one`]
/// or [`WaitQueue::wake_all`] to move sleeping threads back to the `Ready`
/// state so the scheduler can run them again.
pub struct WaitQueue {
    pub state: spin::Mutex<QueueState>,
}

impl WaitQueue {
    /// Creates a new, empty `WaitQueue`.
    pub const fn new() -> Self {
        Self {
            state: spin::Mutex::new(QueueState {
                waiters: alloc::collections::VecDeque::new(),
            }),
        }
    }

    /// Wakes every thread currently waiting on this queue.
    ///
    /// Pops all TIDs from `state.waiters` and calls
    /// [`crate::process::process::Scheduler::wake_task`] on each.  The caller
    /// must already hold a lock on `sched` to avoid re-entrancy.
    pub fn wake_all(&self, sched: &mut crate::process::process::Scheduler) {
        let mut state = self.state.lock();

        while let Some(head_id) = state.waiters.pop_front() {
            sched.wake_task(head_id);
        }
    }

    /// Wakes the first (oldest) thread waiting on this queue, if any.
    ///
    /// Acquires interrupts-disabled → spinlock → scheduler-lock in that order
    /// to avoid priority inversion.  Returns the woken TID, or `None` if
    /// the queue was empty.
    pub fn wake_one(&self) -> Option<usize> {
        x86_64::instructions::interrupts::without_interrupts(|| {
            let head_id = {
                let mut state = self.state.lock();
                state.waiters.pop_front()
            };

            if let Some(id) = head_id {
                let mut sched = crate::process::process::SCHEDULER.lock();
                sched.wake_task(id);
            }

            head_id
        })
    }
}

/// A sleeping mutual-exclusion lock backed by a [`WaitQueue`].
///
/// Unlike a spinlock, `Mutex<T>` puts the calling thread to sleep when the lock
/// is contended, freeing the CPU for other work.  It uses an `AtomicBool` for
/// the lock word and a `WaitQueue` for the blocked-thread list.
///
/// # Implementation
/// 1. Interrupts are disabled to prevent a timer ISR from running the scheduler
///    and producing a TOCTOU race between the `compare_exchange` and the
///    `push_back(current_id)` that registers the thread as a waiter.
/// 2. The thread calls `compare_exchange(false → true)`.  On success the lock
///    is acquired and interrupts are re-enabled.
/// 3. On failure the thread marks itself `Blocked`, adds its TID to the wait
///    queue, and issues `int 0x20` to yield the CPU.  When the lock holder
///    drops the guard, [`MutexGuard::drop`] calls `wake_one` to unblock the
///    next waiter.
pub struct Mutex<T> {
    is_locked: core::sync::atomic::AtomicBool,
    wait_queue: WaitQueue,
    data: core::cell::UnsafeCell<T>,
}

unsafe impl<T: Send> Send for Mutex<T> {}
unsafe impl<T: Send> Sync for Mutex<T> {}

impl<T> Mutex<T> {
    /// Creates a new, unlocked `Mutex` wrapping `data`.
    pub const fn new(data: T) -> Self {
        Self {
            is_locked: core::sync::atomic::AtomicBool::new(false),
            wait_queue: WaitQueue::new(),
            data: core::cell::UnsafeCell::new(data),
        }
    }

    /// Acquires the mutex, blocking the current thread until it is available.
    ///
    /// Spins in a re-try loop: each attempt disables interrupts, checks the
    /// lock word, and either acquires it (re-enables interrupts and returns
    /// the guard) or registers the current thread as a waiter, drops all locks,
    /// re-enables interrupts, and yields with `int 0x20`.
    pub fn lock(&self) -> MutexGuard<'_, T> {
        loop {
            x86_64::instructions::interrupts::disable();

            let mut state = self.wait_queue.state.lock();

            if self
                .is_locked
                .compare_exchange(
                    false,
                    true,
                    core::sync::atomic::Ordering::Acquire,
                    core::sync::atomic::Ordering::Relaxed,
                )
                .is_ok()
            {
                // We got the lock!
                drop(state);
                x86_64::instructions::interrupts::enable();
                return MutexGuard { mutex: self };
            } else {
                let mut sched = crate::process::process::SCHEDULER.lock();

                let current_id = sched.current_task.expect("FATAL: No current task!");
                sched.tasks.get_mut(current_id).unwrap().state =
                    crate::process::process::ThreadState::Blocked;

                state.waiters.push_back(current_id);

                drop(state);
                drop(sched);

                x86_64::instructions::interrupts::enable();
                unsafe {
                    core::arch::asm!("int 0x20");
                }
            }
        }
    }
}

/// RAII guard returned by [`Mutex::lock`].
///
/// Derefs transparently to `T`.  On drop, releases the lock and calls
/// `wake_one` to unblock the next waiting thread.
pub struct MutexGuard<'a, T> {
    mutex: &'a Mutex<T>,
}

// Boilerplate to allow *guard to access the inner data
impl<'a, T> core::ops::Deref for MutexGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.mutex.data.get() }
    }
}
impl<'a, T> core::ops::DerefMut for MutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.mutex.data.get() }
    }
}

// This runs automatically when the Guard goes out of scope
impl<'a, T> Drop for MutexGuard<'a, T> {
    fn drop(&mut self) {
        self.mutex
            .is_locked
            .store(false, core::sync::atomic::Ordering::Release);
        self.mutex.wait_queue.wake_one();
    }
}

/// A counting semaphore that blocks threads when the count reaches zero.
///
/// `Semaphore` is useful for producer/consumer relationships (e.g. the
/// [`crate::sync::ipc::Mailbox`]).  Threads call [`Semaphore::acquire`] to
/// decrement the count (blocking if it is 0) and [`Semaphore::release`] to
/// increment it and wake one waiter.
pub struct Semaphore {
    pub wait_queue: WaitQueue,
    pub count: core::sync::atomic::AtomicUsize,
}

impl Semaphore {
    /// Creates a new `Semaphore` with `initial_count` permits.
    pub const fn new(initial_count: usize) -> Self {
        Self {
            wait_queue: WaitQueue::new(),
            count: core::sync::atomic::AtomicUsize::new(initial_count),
        }
    }

    /// Decrements the semaphore count, blocking if it is currently zero.
    ///
    /// Uses `compare_exchange_weak` in a loop for ABA safety.  On failure
    /// (count == 0) the thread marks itself `Blocked`, registers in the wait
    /// queue, and yields.  Interrupts are disabled around the critical section
    /// to avoid a race between checking the count and sleeping.
    pub fn acquire(&self) {
        loop {
            x86_64::instructions::interrupts::disable();
            let mut state = self.wait_queue.state.lock();

            let count = self.count.load(core::sync::atomic::Ordering::Relaxed);

            if count > 0 {
                if self
                    .count
                    .compare_exchange_weak(
                        count,
                        count - 1,
                        core::sync::atomic::Ordering::Acquire,
                        core::sync::atomic::Ordering::Relaxed,
                    )
                    .is_ok()
                {
                    drop(state);
                    x86_64::instructions::interrupts::enable();
                    return;
                }
            } else {
                // Count is 0. We failed to acquire. Go to sleep.
                let mut sched = crate::process::process::SCHEDULER.lock();
                let current_id = sched.current_task.expect("FATAL: No current task!");

                sched.tasks.get_mut(current_id).unwrap().state =
                    crate::process::process::ThreadState::Blocked;
                state.waiters.push_back(current_id);
                drop(state);
                drop(sched);
                x86_64::instructions::interrupts::enable();
                unsafe {
                    core::arch::asm!("int 0x20");
                }
            }
        }
    }

    /// Increments the semaphore count and wakes one sleeping thread, if any.
    pub fn release(&self) {
        self.count
            .fetch_add(1, core::sync::atomic::Ordering::Release);
        self.wait_queue.wake_one();
    }
}

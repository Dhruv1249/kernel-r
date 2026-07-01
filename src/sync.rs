// src/sync.rs

pub struct QueueState {
    pub waiters: alloc::collections::VecDeque<usize>,
}

pub struct WaitQueue {
    pub state: spin::Mutex<QueueState>,
}

impl WaitQueue {
    pub const fn new() -> Self {
        Self {
            state: spin::Mutex::new(QueueState {
                waiters: alloc::collections::VecDeque::new(),
            }),
        }
    }

    pub fn wake_all(&self, sched: &mut crate::process::Scheduler) {
        let mut state = self.state.lock();

        while let Some(head_id) = state.waiters.pop_front() {
            sched.wake_task(head_id);
        }
    }

    pub fn wake_one(&self) -> Option<usize> {
        x86_64::instructions::interrupts::without_interrupts(|| {
            let head_id = {
                let mut state = self.state.lock();
                state.waiters.pop_front()
            };

            if let Some(id) = head_id {
                let mut sched = crate::process::SCHEDULER.lock();
                sched.wake_task(id);
            }

            head_id
        })
    }
}

pub struct Mutex<T> {
    is_locked: core::sync::atomic::AtomicBool,
    wait_queue: WaitQueue,
    data: core::cell::UnsafeCell<T>,
}

unsafe impl<T: Send> Send for Mutex<T> {}
unsafe impl<T: Send> Sync for Mutex<T> {}

impl<T> Mutex<T> {
    pub const fn new(data: T) -> Self {
        Self {
            is_locked: core::sync::atomic::AtomicBool::new(false),
            wait_queue: WaitQueue::new(),
            data: core::cell::UnsafeCell::new(data),
        }
    }

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
                let mut sched = crate::process::SCHEDULER.lock();

                let current_id = sched.current_task.expect("FATAL: No current task!");
                sched.tasks.get_mut(current_id).unwrap().state =
                    crate::process::ThreadState::Blocked;

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

pub struct Semaphore {
    pub wait_queue: WaitQueue,
    pub count: core::sync::atomic::AtomicUsize,
}

impl Semaphore {
    pub const fn new(initial_count: usize) -> Self {
        Self {
            wait_queue: WaitQueue::new(),
            count: core::sync::atomic::AtomicUsize::new(initial_count),
        }
    }

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
                let mut sched = crate::process::SCHEDULER.lock();
                let current_id = sched.current_task.expect("FATAL: No current task!");

                sched.tasks.get_mut(current_id).unwrap().state =
                    crate::process::ThreadState::Blocked;
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

    pub fn release(&self) {
        self.count
            .fetch_add(1, core::sync::atomic::Ordering::Release);
        self.wait_queue.wake_one();
    }
}

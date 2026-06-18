// src/sync.rs

use core::usize;

struct QueueState {
    head: Option<usize>,
    tail: Option<usize>,
}

pub struct WaitQueue {
    state: spin::Mutex<QueueState>,
}

impl WaitQueue {
    pub const fn new() -> Self {
        Self {
            state: spin::Mutex::new(QueueState {
                head: None,
                tail: None,
            }),
        }
    }

    pub fn wait(&self) {
        let mut sched = crate::process::SCHEDULER.lock();
        let mut state = self.state.lock();

        let current_id = sched.current_task.expect("FATAL: No current task!");
        {
            let current_task = sched.tasks.get_mut(current_id).unwrap();
            current_task.state = crate::process::TaskState::Blocked;
            current_task.next_waiter = None
        }

        if let Some(tail_id) = state.tail {
            let tail_task = sched.tasks.get_mut(tail_id).unwrap();
            tail_task.next_waiter = Some(current_id);
            state.tail = Some(current_id);
        } else {
            state.head = Some(current_id);
            state.tail = Some(current_id);
        }

        drop(sched);
        drop(state);
        // crate::serial_print!("Going to sleep\n");

        unsafe {
            core::arch::asm!("int 0x20");
        }
    }

    pub fn wake_one(&self) -> Option<usize> {
        let mut sched = crate::process::SCHEDULER.lock();
        let mut state = self.state.lock();

        if let Some(head_id) = state.head {
            let next_waiter = {
                let head_task = sched.tasks.get_mut(head_id).unwrap();
                head_task.next_waiter
            };

            state.head = next_waiter;

            if state.head.is_none() {
                state.tail = None;
            }

            sched.wake_task(head_id);
            return Some(head_id);
        } else {
            None
        }
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
            let acquired = x86_64::instructions::interrupts::without_interrupts(|| {
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
                    return true;
                } else {
                    self.wait_queue.wait();
                }
                false
            });

            if acquired {
                return MutexGuard { mutex: self }; // Return from the actual function
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
            let acquired = x86_64::instructions::interrupts::without_interrupts(|| {
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
                        return true;
                    }
                } else {
                    self.wait_queue.wait();
                }
                false
            });

            if acquired {
                return; // Return from the actual function
            }
        }
    }

    pub fn release(&self) {
        self.count
            .fetch_add(1, core::sync::atomic::Ordering::Release);
        self.wait_queue.wake_one();
    }
}

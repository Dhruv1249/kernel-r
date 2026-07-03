//! # Synchronisation Subsystem
//!
//! Provides blocking and non-blocking synchronisation primitives that are safe
//! to use in an interrupt-driven, single-core kernel:
//!
//! | Module | Role |
//! |--------|------|
//! | [`sync`] | `WaitQueue`, sleeping `Mutex<T>`, `MutexGuard<T>`, and `Semaphore` |
//! | [`ipc`] | `Mailbox<T>`: a semaphore-guarded message queue for inter-task communication |
//! | [`queue`] | `RingBuffer<T, N>`: lock-free single-producer / single-consumer ring buffer |

pub mod sync;
pub mod ipc;
pub mod queue;

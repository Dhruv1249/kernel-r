//! # Process Management Subsystem
//!
//! Implements the kernel's threading model and system-call interface:
//!
//! | Module | Role |
//! |--------|------|
//! | [`process`] | `Thread`, `Scheduler` (EEVDF + BORE), `ThreadArena`, timer ISR glue, `spawn`/`join`/`yield_now` |
//! | [`syscall`] | SYSCALL/SYSRET entry stub (assembly) and the Rust `rust_syscall_handler` dispatch table |

pub mod process;
pub mod syscall;

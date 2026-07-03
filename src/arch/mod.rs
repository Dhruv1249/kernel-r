//! # Architecture-Specific Subsystem
//!
//! This module contains all code that is tightly coupled to the underlying CPU
//! architecture. Currently only x86_64 is supported. Placing arch-specific
//! code here makes it easy to add new architectures in the future by simply
//! adding a new sub-module (e.g. `arch/riscv64`).

pub mod x86_64;

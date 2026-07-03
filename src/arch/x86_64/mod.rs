//! # x86_64 Architecture Module
//!
//! Contains all CPU-architecture-specific initialisation code for x86_64:
//! - [`cpu`]: Per-CPU data block and GS-base initialisation.
//! - [`gdt`]: Global Descriptor Table, Task State Segment, and privilege-level
//!   transitions (Ring 0 ↔ Ring 3).

pub mod cpu;
pub mod gdt;

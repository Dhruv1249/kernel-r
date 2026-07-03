//! # Interrupt Handling Subsystem
//!
//! Covers the entire interrupt delivery pipeline on x86_64:
//!
//! | Module | Role |
//! |--------|------|
//! | [`interrupt`] | IDT construction, exception handlers (page fault, GPF, double fault …), and keyboard ISR |
//! | [`apic`] | Local APIC driver: init, EOI signalling, and APIC timer calibration/start |
//! | [`io_apic`] | I/O APIC driver: indirect register access and IRQ redirection table programming |

pub mod interrupt;
pub mod apic;
pub mod io_apic;

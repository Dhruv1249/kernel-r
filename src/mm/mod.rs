//! # Memory Management Subsystem
//!
//! All physical and virtual memory management lives here:
//!
//! | Module | Role |
//! |--------|------|
//! | [`buddy`] | Binary-buddy physical frame allocator (O(log n) alloc/free) |
//! | [`slab`] | Slab-style fixed-size object cache (O(1) alloc/free for small objects) |
//! | [`allocator`] | Global `#[global_allocator]` that stitches slab + buddy together; also houses the interrupt-safe `Locked<T>` wrapper |
//! | [`memory`] | Frame-level public API, bump bootstrap allocator, and heap initialisation |
//! | [`paging`] | 4-level page-table walker: `map_to`, `unmap`, `translate_addr`, and user sandbox setup |

pub mod buddy;
pub mod slab;
pub mod allocator;
pub mod memory;
pub mod paging;

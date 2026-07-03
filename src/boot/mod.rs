//! # Boot Information Parsing
//!
//! Parses the Multiboot2 information structure passed by GRUB and exposes
//! typed accessors for memory-map entries, ACPI RSDP tags, MADT, and I/O APIC
//! records. All structs mirror the Multiboot2 specification layout exactly so
//! they can be read directly from raw memory with no deserialization overhead.

pub mod boot_info;

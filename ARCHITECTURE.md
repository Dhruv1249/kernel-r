# Kernel-r Architecture

## Overview

Kernel-r is a small Rust-based kernel targeted at x86_64. It includes a boot sequence via GRUB, a Rust runtime with minimal init, memory management, interrupt handling with APIC support, simple process scheduling, and basic drivers (serial, VGA, keyboard).

## Boot sequence

- GRUB loads the kernel from `kernel.elf` (Multiboot2 configuration in `iso/boot/grub/grub.cfg`).
- Early assembly in `src/boot.asm` sets up initial CPU state and jumps to the Rust entry (`lib.rs` / `start`).

## Memory layout

- Physical memory is managed by a physical allocator (see `src/allocator.rs`, `src/buddy.rs`, `src/slab.rs`).
- Virtual memory uses x86_64 paging (see `src/paging.rs`) with a higher-half kernel mapping.
- `target.json` configures a bare-metal x86_64 target used by the build system.

## Allocator

- The project contains multiple allocator components:
  - Buddy allocator (`src/buddy.rs`) for managing physical frames.
  - Slab-like allocator (`src/slab.rs`) for small object reuse.
  - `src/allocator.rs` provides the kernel-facing allocation interface.

## Paging and virtual memory

- Page table setup and mapping helpers live in `src/paging.rs`.
- Kernel reserves a virtual region for the higher-half mapping; physical frames are mapped using the page table helpers.

## Interrupts and APIC

- Local APIC / I/O APIC support in `src/apic.rs` and `src/io_apic.rs`.
- Interrupt vectors and handlers are defined in `src/interrupt.rs`.

## Processes and scheduling

- Basic process structures and scheduler are provided in `src/process.rs`.
- Inter-process queues and message passing live in `src/queue.rs`.

## Drivers and subsystems

- Console/graphics: `src/vga_buffer.rs`
- Serial: `src/serial.rs`
- Keyboard: `src/keyboard.rs`
- Timer / scheduler integrations: `src/apic.rs` and `src/interrupt.rs`

## Build and release

- Build script: `./build.sh` (links with LLD and creates `kernel.elf` and `kernel.iso`).
- Release artifacts to attach: `kernel.iso`, `kernel.elf`.
- Recommended: publish checksums (SHA256) alongside release artifacts.

## Extensibility

- To add a new architecture, create an `arch/` subdirectory with an appropriate `target.json` and adapt `build.sh`.

---


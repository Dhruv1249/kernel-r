# kernel-r

A bare metal x86_64 kernel written in Rust, booted via GRUB.

## Prerequisites

### Rust Toolchain

Install `rustup` (do NOT use pacman's rust):
# Kernel-r

Kernel-r is a small educational x86_64 kernel written in Rust. It targets bare-metal x86_64 and provides a minimal kernel runtime, basic memory management, interrupt handling, and simple process primitives for experimentation and research.

## Status
- Target: x86_64 (see `target.json` and `iso/boot/grub/grub.cfg`)
- Release: v0.1.0
- Not production-ready; intended for learning and research

## Quick start

1. Clone the repository:

```bash
git clone https://github.com/<your-username>/kernel-r.git
cd kernel-r
```

2. Build (project provides a build script):

```bash
./build.sh
```

3. Output artifacts:

- `kernel.iso` — bootable ISO
- `kernel.elf` — ELF kernel image

## Project structure

- `src/` — kernel sources (allocator, paging, process, drivers)
- `iso/` — GRUB / ISO layout
- `build.sh` — build + link + iso creation script
- `target.json` and `.cargo/config.toml` — cross-target configuration for x86_64

## Building and release

The repository includes a build script that compiles the kernel and produces release artifacts. For automated releases, a GitHub Actions workflow is provided to build and attach `kernel.iso` and `kernel.elf` to a release tag.

## Release v0.1.0 notes

See `ARCHITECTURE.md` for architecture details included in the release notes.

## Contributing

- Open issues or pull requests for bug reports or features.
- Follow existing code style and include tests where applicable.


## Running

```bash
./build.sh --run
```


## Notes

- The kernel is compiled for a custom `x86_64-unknown-none` target with SSE/MMX disabled and red zone disabled — both required for correct kernel operation
- GRUB hands off execution in 32-bit protected mode; a boot assembly stub is needed to switch to 64-bit long mode before Rust code runs

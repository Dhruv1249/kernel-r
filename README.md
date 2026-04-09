# kernel-r

A bare metal x86_64 kernel written in Rust, booted via GRUB.

## Prerequisites

### Rust Toolchain

Install `rustup` (do NOT use pacman's rust):

```bash
# Remove pacman rust if installed
sudo pacman -Rns rust rust-src

# Install rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Install nightly toolchain (required for build-std)
rustup install nightly
rustup override set nightly

# Add rust source (required for build-std)
rustup component add rust-src --toolchain nightly-x86_64-unknown-linux-gnu
```

### System Packages

```bash
sudo pacman -S grub qemu-system-x86 xorriso mtools tigervnc
```

| Package | Purpose |
|---|---|
| `grub` | Bootloader, used to create bootable ISO |
| `qemu-system-x86` | x86_64 emulator to run the kernel |
| `xorriso` | Required by `grub-mkrescue` to create ISOs |
| `mtools` | Required by `grub-mkrescue` for FAT image creation |
| `tigervnc` | VNC viewer to see QEMU output |

## Project Structure

```
kernel-r/
├── .cargo/
│   └── config.toml       # build target + build-std config
├── src/
│   └── main.rs           # kernel entry point
├── target.json           # custom bare metal x86_64 target
├── linker.ld             # linker script
├── build.sh              # build + ISO creation script
├── Cargo.toml
└── .gitignore
```

## Building

```bash
./build.sh
```

This will:
1. Compile the kernel using the custom `target.json`
2. Create a bootable ISO with GRUB at `kernel.iso`

## Running

```bash
# Start QEMU (opens VNC server on localhost:5900)
qemu-system-x86_64 -cdrom kernel.iso

# In another terminal, connect to see output
vncviewer localhost:5900
```

## Config Files

### `.cargo/config.toml`

```toml
[unstable]
build-std = ["core", "compiler_builtins"]
build-std-features = ["compiler-builtins-mem"]

[build]
target = "target.json"

[target.'cfg(target_os = "none")']
rustflags = ["-C", "link-arg=-Tlinker.ld"]
```

### `Cargo.toml`

```toml
[package]
name = "kernel-r"
version = "0.1.0"
edition = "2021"

[dependencies]

[profile.dev]
panic = "abort"

[profile.release]
panic = "abort"

[[bin]]
name = "kernel-r"
test = false
bench = false
```

## Current Status

- [x] Boots via GRUB
- [x] Multiboot2 header
- [x] VGA text output
- [ ] 32-bit to 64-bit long mode switch (assembly boot stub)
- [ ] Interrupt handling (GDT, IDT)
- [ ] Memory management

## Notes

- The kernel is compiled for a custom `x86_64-unknown-none` target with SSE/MMX disabled and red zone disabled — both required for correct kernel operation
- GRUB hands off execution in 32-bit protected mode; a boot assembly stub is needed to switch to 64-bit long mode before Rust code runs
- `serde_core` is a malicious crate — never add it as a dependency. Use real `serde` with `default-features = false` if serialization is needed

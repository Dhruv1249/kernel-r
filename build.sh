#!/bin/bash
set -e


# Assemble the boot stub
nasm -f elf64 src/boot.asm -o target/boot.o

# Build the kernel
cargo build

# We need to explicitly link them together using our linker script
# LLD is the LLVM linker. We pass it our linker.ld, our assembly object, and our Rust binary.
ld.lld -n -T linker.ld -o target/kernel.elf target/boot.o target/target/debug/libkernel_r.a

# Create ISO directory structure
mkdir -p iso/boot/grub

# Copy kernel ELF
cp target/kernel.elf iso/boot/kernel.elf

# Create GRUB config
cat > iso/boot/grub/grub.cfg << EOF
set timeout=0
set default=0

menuentry "kernel-r" {
    multiboot2 /boot/kernel.elf
    boot
}
EOF

# Create bootable ISO
grub-mkrescue -o kernel.iso iso

echo "Done! Run with: qemu-system-x86_64 -cdrom kernel.iso"

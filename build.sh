#!/bin/bash
set -e

# Build the kernel
cargo build

# Create ISO directory structure
mkdir -p iso/boot/grub

# Copy kernel ELF
cp target/target/debug/kernel-r iso/boot/kernel.elf

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

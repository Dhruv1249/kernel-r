#!/bin/bash

istest=0
isRun=0
somethingElse=0
for arg in "$@"; do
  case "$arg" in
  --test)
    istest=1
    ;;
  --run)
    isRun=1
    ;;
  *)
    somethingElse=1
    ;;
  esac
done

if [ "$isRun" -eq 1 ] && [ "$istest" -eq 1 ]; then
  echo "Invalid arguments both run and build can't be used together"
  exit 1 
elif [ "$somethingElse" -eq 1 ]; then
  echo "Invalid arguments"
  exit 1
fi

set -e

# Make sure we have target directory
mkdir -p target

# Assemble the boot stub
nasm -f elf64 src/boot.asm -o target/boot.o

# Compile the C Red-Black Tree logic
# -ffreestanding: No standard library
# -mno-red-zone: CRITICAL for kernel code to prevent hardware interrupts from trashing the stack
clang -target x86_64-unknown-none -ffreestanding -mno-red-zone -c src/rbtree.c -o target/rbtree.o

# Build the kernel
if [ "$istest" -eq 1 ]; then
  cargo build --features test
else
  cargo build
fi

# We need to explicitly link them together using our linker script
# LLD is the LLVM linker. We pass it our linker.ld, our assembly object, and our Rust binary.
ld.lld -n -T linker.ld -o target/kernel.elf target/boot.o target/target/debug/libkernel_r.a target/rbtree.o

# Create ISO directory structure
mkdir -p iso/boot/grub

# Copy kernel ELF
cp target/kernel.elf iso/boot/kernel.elf

# Create GRUB config
cat >iso/boot/grub/grub.cfg <<EOF
set timeout=0
set default=0

menuentry "kernel-r" {
    multiboot2 /boot/kernel.elf
    boot
}
EOF

# Create bootable ISO
grub-mkrescue -o kernel.iso iso

if [ "$isRun" -eq 0 ] && [ "$istest" -eq 0 ]; then
  echo "Build complete"
elif [ "$isRun" -eq 1 ]; then
  qemu-system-x86_64 -enable-kvm -cpu host -cdrom kernel.iso
else
  qemu-system-x86_64 -enable-kvm -cpu host -device isa-debug-exit,iobase=0xf4,iosize=0x04 -serial stdio -cdrom kernel.iso
fi

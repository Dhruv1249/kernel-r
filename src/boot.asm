
; Since grub has a 32 bit environment, we must start in 32 bit mode

bits 32

section .text
; Global start label tells the linker that this is the entry point
global start

start:
  ; Magic number put in eax by grub by default
  ; We are just checking if we are booting successfully
  cmp eax, 0x36d76289
  jne .no_multiboot

  hlt

.no_multiboot:
  hlt ; Something went wrong, halt the system






	; Since grub has a 32 bit environment, we must start in 32 bit mode

	bits 32

	section .text
	;       Global start label tells the linker that this is the entry point
	global  start

start:
	;   Magic number put in eax by grub by default
	;   We are just checking if we are booting successfully
	cmp eax, 0x36d76289
	jne .no_multiboot

	;   We are now gonna check if the cpu supports cpuid
	;   CPUID is stored in bit 21 of the EFLAGS register
	;   In older cpus eflags were hardcoded this means if we flip the bit 21 and push it
	;   back to elfags it should still stay the same if it sticks the cpu supports cpuid
	;   Push eflags register onto the stack
	;   EFLAGS is not a general purpose register, it is a special register
	pushfd
	pop eax; Put eflags from stack into eax
	xor eax, 1<<21; Flip bit 21 of eax

	push  eax
	popfd ; Put eax back into eflags register

	pushfd
	pop ebx

	;   Compare the two flags, if they are the same then the cpu supports cpuid
	cmp ebx, eax
	jne .no_multiboot; Not equal, so we are not booting with multiboot

  
  ; Calling cpuid
	mov eax, 0x80000001 ; Magic number to call cpuid
	cpuid ; Call cpuid and put eflags into edx and eax gets filled with cpu family and model

  test edx, 1 << 29
  jz .no_multiboot

	hlt

.no_multiboot:
	hlt ; Something went wrong, halt the system


	; Since grub has a 32 bit environment, we must start in 32 bit mode

	bits 32

	;      Telling asm about our rust entry point
	extern _start

	section .text
	;       Global start label tells the linker that this is the entry point
	global  start

	;       Magic code for multiboot
	section .multiboot2

header_start:
	dd 0xe85250d6; magic
	dd 0; architecture (i386)
	dd header_end - header_start; header length
	dd 0x100000000 - (0xe85250d6 + 0 + (header_end - header_start)); checksum
	;  End tag — required, tells GRUB no more tags follow
	dw 0; type = 0 (end tag)
	dw 0; flags
	dd 8; size = 8 bytes

header_end:

start:
	;   Magic number put in eax by grub by default
	;   We are just checking if we are booting successfully
	cmp eax, 0x36d76289
	jne no_multiboot

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
	jne no_multiboot; Not equal, so we are not booting with multiboot

	;     Calling cpuid
	mov   eax, 0x80000001; Magic number to call cpuid
	cpuid ; Call cpuid and put eflags into edx and eax gets filled with cpu family and model

	;    Hers test sets all the bits except the 29th bit to zero
	;    If the 29th bit is 1, then the cpu is 64 bit
	test edx, 1 << 29
	jz   no_multiboot; Halt if not 64 bit

	call set_up_page_tables

	mov eax, p4_table
	mov cr3, eax; Give access of page table to the processor

	;   Enabling PAE (Physical Address Extension)
	mov eax, cr4
	or  eax, 1 << 5; Set the PAE bit
	mov cr4, eax

	;   Enabling Long Mode
	;   It lives in MSR (Model Specific Register) called the EFER register
	;   To access it we use the RDMSR and WRMSR instructions it reads instructions from ecx
	;   Long mode lives in the 8th bit
	;   Magic number
	mov ecx, 0xC0000080
	rdmsr
	or  eax, 1 << 8
	wrmsr

	;   Enabling paging
	;   It lives in the 31st bit of CR0
	mov eax, cr0
	or  eax, 1 << 31
	mov cr0, eax

	;    lgdt -> Load Global Descriptor Table
	lgdt [gdt64.pointer]

	;   Far jump to the long mode code
	jmp gdt64.code_segment:long_mode_start

hlt

no_multiboot:
	hlt ; Something went wrong, halt the system

set_up_page_tables:
	;   Linking the page tables to the correct location
	;   For all levels we are setting bit 0 to 1 to indicate that the entry is present
	;   Same for bit 1 to indicate it is writable
	mov eax, p3_table
	;   Or to set last 2 bits to 1
	or  eax, 0b11
	mov [p4_table], eax
	mov eax, p2_table
	or  eax, 0b11
	mov [p3_table], eax

	mov ecx, 0

	; Mapping all the pages since our kernel was bigger than 2mb

.map_p2_table:
	;    Calculate the physical address of the 2mb page
	imul eax, ecx, 0x200000
	or   eax, 0b10000011; Add flags
	mov  [p2_table+ ecx * 8], eax; Write the entry
	inc  ecx
	cmp  ecx, 512
	jne  .map_p2_table

	ret

bits 64

long_mode_start:
	;   1. Clear out the old 32-bit segment registers by setting them to 0
	mov ax, 0
	mov ss, ax
	mov ds, ax
	mov es, ax
	mov fs, ax
	mov gs, ax

	;   2. Point the CPU's Stack Pointer to our new stack
	mov rsp, stack_top

	;   3. Now it is safe to jump into Rust!
	jmp _start

section .bss

	;     Defining page tables without it cpu won't start in 64 bit mode
	;     Fun fact for our 4 level page table theoretical ram limit would be 256 TB
	;     Page tables must be 4kb aligned
	align 4096

p4_table:
	resb 4096; Reserve 4kb of memory

p3_table:
	resb 4096

p2_table:
	resb 4096

stack_bottom:
	resb 4096 * 4; Reserve 16 KB for the stack

stack_top:

	;       Some magic code for GDT (Global Descriptor Table)
	section .rodata

gdt64:
	dq 0; Entry 0: The Null Descriptor (CPU mandates the first entry be completely zero)

	.code_segment: equ $ - gdt64
	;  Entry 1: The Code Descriptor
	;  Bit 43 = Executable, Bit 44 = Code/Data type, Bit 47 = Present, Bit 53 = 64-bit flag
	dq (1<<43) | (1<<44) | (1<<47) | (1<<53)

.pointer:
	dw $ - gdt64 - 1; Length of the GDT minus 1
	dq gdt64; Memory address of the GDT

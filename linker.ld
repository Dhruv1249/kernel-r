ENTRY(start)

/* This is where we want our kernel to think it lives*/
KERNEL_OFFSET = 0xffff800000000000;

/* GRUB will physically load kernel at this address */
PHYS_LOAD_ADDR = 1M;

SECTIONS {
    . = PHYS_LOAD_ADDR;

    /* GRUB requires the multiboot header to be sit at lower address */
    .boot :
    {
        KEEP(*(.multiboot2))
        *(.boot.text)
        *(.boot.rodata)
        *(.boot.bss)
    }

    /* We need to move the kernel to a higher address */
    . += KERNEL_OFFSET;

    kernel_start = .; /* Mark the start of the kernel */

    /* For all sections below, we use AT(ADDR(.section) - KERNEL_OFFSET) 
       This tells the linker: "Pretend this is at 0xFFFF8000..., but actually 
       put the raw bytes down at 1 MB." */
    .text ALIGN(4K) : AT(ADDR(.text) - KERNEL_OFFSET)
    {
        *(.text .text.*)
    }

    .rodata ALIGN(4K) : AT(ADDR(.rodata) - KERNEL_OFFSET)
    {
        *(.rodata .rodata.*)
    }

    .data ALIGN(4K) : AT(ADDR(.data) - KERNEL_OFFSET)
    {
        *(.data .data.*)
    }

    .bss ALIGN(4K) : AT(ADDR(.bss) - KERNEL_OFFSET)
    {
        *(.bss .bss.*)
    }

    /* Align to 4Kb so the our allocator starts with a clean page */
    . = ALIGN(4K);
    kernel_end = .; /* Mark the end of the kernel */
    /DISCARD/ : {
    *(.eh_frame)
    *(.eh_frame_hdr)
    }
}

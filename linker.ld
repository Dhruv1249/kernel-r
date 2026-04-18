ENTRY(start)

SECTIONS {
    . = 1M;

    kernel_start = .; /* Mark the start of the kernel */

    .boot :
    {
        KEEP(*(.multiboot2))
    }

    .text :
    {
        *(.text .text.*)
    }

    .rodata :
    {
        *(.rodata .rodata.*)
    }

    .data :
    {
        *(.data .data.*)
    }

    .bss :
    {
        *(.bss .bss.*)
    }

    /* Align to 4Kb so the our allocator starts with a clean page */
    . = ALIGN(4K);
    kernel_end = .; /* Mark the end of the kernel */
}

OUTPUT_ARCH(riscv)
ENTRY(_start)

ASSERT(SERVER_HEAP_SIZE > 0, "SERVER_HEAP_SIZE must be nonzero")
ASSERT((SERVER_HEAP_SIZE % 4096) == 0, "SERVER_HEAP_SIZE must be page-aligned")

SECTIONS {
    . = DEFINED(SERVER_LOAD_ADDRESS) ? SERVER_LOAD_ADDRESS : 0x10000;
    __server_image_start = .;
    ASSERT((__server_image_start % 4096) == 0, "server load address must be page-aligned")

    .text : {
        KEEP(*(.text.entry))
        *(.text .text.*)
    }

    .rodata ALIGN(4096) : { *(.rodata .rodata.* .srodata .srodata.*) }

    .data ALIGN(4096) : {
        PROVIDE(__global_pointer$ = . + 0x800);
        *(.data .data.* .sdata .sdata.*)
    }
    .bss (NOLOAD) : {
        __server_bss_start = .;
        *(.bss .bss.* .sbss .sbss.*)
        *(COMMON)
        __server_bss_end = .;
    }

    .server_heap ALIGN(4096) (NOLOAD) : {
        __server_heap_start = .;
        . += SERVER_HEAP_SIZE;
        __server_heap_end = .;
    }
    __server_image_end = .;
}

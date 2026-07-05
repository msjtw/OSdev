OUTPUT_ARCH("riscv")

ENTRY(_entry)

MEMORY {
  ram (rwx) : ORIGIN = 0x80200000, LENGTH = 62M
}

SECTIONS {
  .text : {
    *(.text.boot)
    *(.text .text.*)

    . = ALIGN(0x1000);

    PROVIDE(_trampoline = .);

    KEEP(*(.trampsec))

    . = ALIGN(0x1000);

    ASSERT(. - _trampoline >= 0x0, "trampoline empty");
    ASSERT(. - _trampoline == 0x1000, "trampoline too large");

    PROVIDE(etext = .);
  } > ram

  .rodata : {
    *(.rodata*)
    . = ALIGN(16);
  } > ram

  .data : {
    *(.data*)
    . = ALIGN(16);
  } > ram

  .bss : {
    *(.bss*)
    . = ALIGN(16);
  } > ram

  . = ALIGN(0x1000); 
  . = . + 0x1000;
  PROVIDE(_STACK_PTR = .);
  /* . = . + 32; */
  . = ALIGN(0x1000); 
  PROVIDE(ekernel = .);
}

OUTPUT_ARCH("riscv")

ENTRY(_entry)

MEMORY {
  ram (rwx) : ORIGIN = 0x80200000, LENGTH = 62M
}

SECTIONS {
  .text.boot : {
    *(.text.boot)
    . = ALIGN(4);
  } > ram

  .text : {
    *(.text)
    . = ALIGN(4);
  } > ram

  .trampsec : ALIGN(0x1000)
  {
    _trampoline = .;
    KEEP(*(.trampsec))
    . = ALIGN(0x1000);
    ASSERT(. - _trampoline != 0x1000, "trampoline too large");
  }
  PROVIDE(etext = .);

  .rodata : {
    *(.rodata*)
    . = ALIGN(4);
  } > ram

  .data : {
    *(.data*)
    . = ALIGN(4);
  } > ram

  .bss : {
    *(.bss*)
    . = ALIGN(4);
  } > ram

  . = ALIGN(8);
  . = . + 4096;
  PROVIDE(_STACK_PTR = .);
  . = . + 32;
  . = ALIGN(0x1000); 
  PROVIDE(ekernel = .);
}

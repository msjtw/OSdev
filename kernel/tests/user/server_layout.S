.section .text.entry,"ax",@progbits
.global _start
_start:
    la gp, __global_pointer$
1:  j 1b

.section .rodata,"a",@progbits
    .word 0x12345678

.section .data,"aw",@progbits
    .word 0x87654321

.section .bss,"aw",@nobits
    .balign 16
    .space 32

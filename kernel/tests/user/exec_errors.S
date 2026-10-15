/* Linux RV32 ABI regression: failed exec must return to this same process.
 * CASE: 1 bad path pointer, 2 bad argv pointer, 3 bad argument pointer,
 *       4 too many arguments, 5 oversized argument, 6 malformed ELF.
 */
.section .text
.global _start
_start:
    li s0, 20
again:
    la a0, truepath
    la a1, arguments
    li a2, 0                  /* No environment variables. */
#if CASE == 1
    li a0, 1
#elif CASE == 2
    li a1, 1
#elif CASE == 3
    la a1, invalid_arguments
#elif CASE == 4
    la a1, many_arguments
#elif CASE == 5
    la a1, large_arguments
#elif CASE == 6
    la a0, badelf
#else
#error Unknown CASE
#endif
    li a7, 221
    ecall
#if CASE <= 3
    li t0, -14                /* EFAULT */
#elif CASE <= 5
    li t0, -7                 /* E2BIG */
#else
    li t0, -8                 /* ENOEXEC */
#endif
    bne a0, t0, failed
    addi s0, s0, -1
    bnez s0, again

    li a0, 1
    la a1, success
    li a2, 18
    li a7, 64
    ecall
    /* The original process can still successfully replace its image. */
    la a0, truepath
    la a1, arguments
    li a2, 0
    li a7, 221
    ecall
failed:
    li a0, 1
    li a7, 93
    ecall
    j .

.section .rodata
truepath:
    .asciz "/bin/true"
badelf:
    .asciz "/m007-bad-elf"
success:
    .ascii "M007_EXEC_TEST_OK\n"
large_string:
    .fill 16384, 1, 120
    .byte 0
.balign 4
arguments:
    .word truepath, 0
invalid_arguments:
    .word 1, 0
large_arguments:
    .word large_string, 0
many_arguments:
    .rept 65
    .word truepath
    .endr
    .word 0

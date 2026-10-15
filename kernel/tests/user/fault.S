/* Standalone RV32 Linux-ABI test. No libc or kernel test hooks required.
 * FAULT_KIND: 1 = load page fault, 2 = store page fault, 3 = illegal opcode.
 * Parent verifies signal termination and reaping over more than 32 children.
 */
.section .text
.global _start
_start:
    li s0, 40
again:
    li a0, 17                  /* clone(SIGCHLD), fork-style */
    li a1, 0
    li a2, 0
    li a3, 0
    li a4, 0
    li a7, 220
    ecall
    bltz a0, failed
    beqz a0, child
    mv s1, a0

    li a0, 0                   /* waitid(P_ALL, 0, &info, WEXITED, NULL) */
    li a1, 0
    la a2, info
    li a3, 4
    li a4, 0
    li a7, 95
    ecall
    bnez a0, failed
    la t0, info
    lw t1, 0(t0)               /* si_signo == SIGCHLD */
    li t2, 17
    bne t1, t2, failed
    lw t1, 8(t0)               /* si_code == CLD_KILLED */
    li t2, 2
    bne t1, t2, failed
    lw t1, 12(t0)              /* si_pid is this child */
    bne t1, s1, failed
    lw t1, 20(t0)              /* si_status is the terminating signal */
#if FAULT_KIND == 3
    li t2, 4                   /* SIGILL */
#else
    li t2, 11                  /* SIGSEGV */
#endif
    bne t1, t2, failed

    li a0, 0                   /* Reaping must leave no children. */
    li a1, 0
    la a2, info
    li a3, 5                   /* WEXITED | WNOHANG */
    li a4, 0
    li a7, 95
    ecall
    li t0, -10                 /* -ECHILD */
    bne a0, t0, failed
    addi s0, s0, -1
    bnez s0, again

    li a0, 1
    la a1, success
    li a2, 19
    li a7, 64
    ecall
    li a0, 0
    j exit

child:
#if FAULT_KIND == 1
    li t0, 0
    lw t1, 0(t0)
#elif FAULT_KIND == 2
    li t0, 0
    sw zero, 0(t0)
#elif FAULT_KIND == 3
    .word 0                    /* architecturally illegal instruction */
#else
#error Unknown FAULT_KIND
#endif
failed:
    li a0, 1
exit:
    li a7, 93
    ecall
    j .

.section .rodata
success:
    .ascii "M006_FAULT_TEST_OK\n"

.section .bss
.balign 4
info:
    .space 128

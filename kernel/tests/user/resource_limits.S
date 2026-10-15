/* Test only fixed process/FD limits and reuse, not memory exhaustion. */
.section .text
.global _start
_start:
#if CASE == 4
    andi t0, sp, 15
    bnez t0, failed
    lw t0, 0(sp)               /* argc still lives at the aligned SP */
    beqz t0, failed
    lw t1, 4(sp)               /* argv[0] remains accessible */
    lbu t2, 0(t1)
    beqz t2, failed
#elif CASE == 3
    li a0, 17
    li a1, 0
    li a2, 0
    li a3, 0
    li a4, 0
    li a7, 220
    ecall
    bltz a0, failed
    beqz a0, sleeping_child
    li a0, 0                   /* Live child: WNOHANG must not sleep/lock. */
    li a1, 0
    la a2, info
    li a3, 5
    li a4, 0
    li a7, 95
    ecall
    bnez a0, failed
    la t0, info
    lw t1, 0(t0)
    bnez t1, failed            /* No child exit has been reported. */
    call wait_child            /* Must still be able to block, then reap. */
#elif CASE == 1
    li s0, 0
fork_more:
    li a0, 17
    li a1, 0
    li a2, 0
    li a3, 0
    li a4, 0
    li a7, 220
    ecall
    beqz a0, success_exit
    bltz a0, exhausted
    addi s0, s0, 1
    li t0, 32
    bge s0, t0, failed
    j fork_more
exhausted:
    li t0, -11                 /* EAGAIN */
    bne a0, t0, failed
    li t0, 30                  /* 32 minus shell and this test parent */
    bne s0, t0, failed
reap:
    call wait_child
    addi s0, s0, -1
    bnez s0, reap
    /* A reaped slot must be usable by a subsequent clone. */
    li a0, 17
    li a1, 0
    li a2, 0
    li a3, 0
    li a4, 0
    li a7, 220
    ecall
    beqz a0, success_exit
    bltz a0, failed
    call wait_child
#elif CASE == 2
    li s0, 3
open_more:
    call open_file
    bne a0, s0, failed
    addi s0, s0, 1
    li t0, 64
    blt s0, t0, open_more
    call open_file
    li t0, -24                 /* EMFILE */
    bne a0, t0, failed
    li a0, 0
    li a7, 23                  /* dup must obey the same full-table limit */
    ecall
    li t0, -24
    bne a0, t0, failed
    li a0, 0
    li a1, 64
    li a2, 0
    li a7, 24                  /* dup3 cannot allocate descriptor 64 */
    ecall
    li t0, -24
    bne a0, t0, failed

    call close_seven
    call open_file
    li t0, 7
    bne a0, t0, failed
    call close_seven
    li a0, 0
    li a7, 23
    ecall
    li t0, 7
    bne a0, t0, failed
    call close_seven
    li a0, 0
    li a1, 7
    li a2, 0
    li a7, 24
    ecall
    li t0, 7
    bne a0, t0, failed
    /* Reuse the last slot too (exercises table-tail removal). */
    li a0, 63
    li a7, 57
    ecall
    bnez a0, failed
    call open_file
    li t0, 63
    bne a0, t0, failed
#else
#error Unknown CASE
#endif
    li a0, 1
    la a1, message
    li a2, 20
    li a7, 64
    ecall
success_exit:
    li a0, 0
    j exit
failed:
    li a0, 1
exit:
    li a7, 93
    ecall
    j .

wait_child:
    li a0, 0
    li a1, 0
    la a2, info
    li a3, 4
    li a4, 0
    li a7, 95
    ecall
    bnez a0, failed
    ret
open_file:
    li a0, -100
    la a1, path
    li a2, 0
    li a3, 0
    li a7, 56
    ecall
    ret
close_seven:
    li a0, 7
    li a7, 57
    ecall
    bnez a0, failed
    ret
sleeping_child:
    li a0, 1                   /* clock_nanosleep_time64(CLOCK_MONOTONIC) */
    li a1, 0
    la a2, delay
    li a3, 0
    li a7, 407
    ecall
    j success_exit
.section .rodata
 .balign 8
delay:
    .dword 1, 0                /* one second, zero nanoseconds */
path:
    .asciz "/lorem.txt"
message:
    .ascii "M008_LIMITS_TEST_OK\n"
.section .bss
.balign 4
info:
    .space 128

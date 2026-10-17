/* M-022: descriptor sharing, lseek, dup3, fcntl FD_CLOEXEC, and exec. */
.section .text
.global _start
_start:
#if ROLE == 2
    /* This image is reached only after fd 3 was marked close-on-exec. */
    li a0, 3
    la a1, byte
    li a2, 1
    li a7, 63                 /* read */
    ecall
    li t0, -9                 /* EBADF */
    bne a0, t0, role2_failed
    li a0, 1
    la a1, success
    li a2, 16
    li a7, 64
    ecall
    li a0, 0
    li a7, 93
    ecall
    j .
role2_failed:
    li a0, 1
    li a7, 93
    ecall
    j .
#else
    li a0, -100
    la a1, lorem
    li a2, 0
    li a3, 0
    li a7, 56                 /* openat -> fd 3 */
    ecall
    li t0, 3
    bne a0, t0, failed

    li a0, 3
    la a1, byte
    li a2, 1
    li a7, 63
    ecall
    li t0, 1
    bne a0, t0, failed

    li a0, 3
    li a7, 23                 /* dup -> fd 4 */
    ecall
    li t0, 4
    bne a0, t0, failed

    li a0, 4
    la a1, byte
    li a2, 1
    li a7, 63
    ecall
    li t0, 1
    bne a0, t0, failed
    la t0, byte
    lbu t1, 0(t0)
    li t2, 'o'                /* "Lorem": duplicated fd read byte two. */
    bne t1, t2, failed

    li a0, 3
    li a1, 0
    li a2, 0                  /* SEEK_SET */
    li a7, 62                 /* lseek */
    ecall
    bnez a0, failed

    li a0, 4
    la a1, byte
    li a2, 1
    li a7, 63
    ecall
    li t0, 1
    bne a0, t0, failed
    la t0, byte
    lbu t1, 0(t0)
    li t2, 'L'
    bne t1, t2, failed

    li a0, 3
    li a1, 4
    li a2, 0
    li a7, 24                 /* dup3 replaces occupied fd 4 */
    ecall
    li t0, 4
    bne a0, t0, failed

    li a0, 3
    li a1, 1                  /* F_GETFD */
    li a2, 0
    li a7, 25                 /* fcntl */
    ecall
    bnez a0, failed
    li a0, 3
    li a1, 2                  /* F_SETFD */
    li a2, 1                  /* FD_CLOEXEC */
    li a7, 25
    ecall
    bnez a0, failed
    li a0, 3
    li a1, 1
    li a2, 0
    li a7, 25
    ecall
    li t0, 1
    bne a0, t0, failed

    li a0, 3
    li a7, 23                 /* dup clears FD_CLOEXEC on new fd */
    ecall
    li t0, 5
    bne a0, t0, failed
    li a0, 5
    li a1, 1
    li a2, 0
    li a7, 25
    ecall
    bnez a0, failed

    la a0, after
    la a1, argv
    li a2, 0
    li a7, 221                /* execve */
    ecall
failed:
    li a0, 1
exit:
    li a7, 93
    ecall
    j .
#endif
.section .rodata
lorem:
    .asciz "/lorem.txt"
after:
    .asciz "/m022-after"
.balign 4
argv:
    .word after, 0
success:
    .ascii "M022_FD_TEST_OK\n"
.section .bss
.balign 4
byte:
    .space 1

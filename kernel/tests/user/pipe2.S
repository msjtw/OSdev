/* M-030: pipe2 ABI, EOF, EPIPE, flags, and invalid output pointers. */
.section .text
.global _start
_start:
    li a0, 0
    li a1, 0
    li a7, 59                 /* pipe2 invalid destination */
    ecall
    li t0, -14                /* EFAULT */
    bne a0, t0, failed

    la a0, fds
    li a1, 1
    li a7, 59                 /* flags must be zero */
    ecall
    li t0, -22                /* EINVAL */
    bne a0, t0, failed

    la a0, fds
    li a1, 0
    li a7, 59
    ecall
    bnez a0, failed
    la t0, fds
    lw s0, 0(t0)              /* read end */
    lw s1, 4(t0)              /* write end */
    li t1, 3                  /* failed pipe2 did not leak descriptors */
    bne s0, t1, failed
    li t1, 4
    bne s1, t1, failed

    mv a0, s1
    la a1, byte_x
    li a2, 1
    li a7, 64                 /* write */
    ecall
    li t0, 1
    bne a0, t0, failed

    mv a0, s1
    li a7, 57                 /* close last writer */
    ecall
    bnez a0, failed

    mv a0, s0
    la a1, byte
    li a2, 1
    li a7, 63                 /* read data */
    ecall
    li t0, 1
    bne a0, t0, failed
    la t0, byte
    lbu t1, 0(t0)
    li t2, 'x'
    bne t1, t2, failed

    mv a0, s0
    la a1, byte
    li a2, 1
    li a7, 63                 /* EOF after last writer closes */
    ecall
    bnez a0, failed
    mv a0, s0
    li a7, 57
    ecall
    bnez a0, failed

    la a0, fds
    li a1, 0
    li a7, 59
    ecall
    bnez a0, failed
    la t0, fds
    lw s0, 0(t0)
    lw s1, 4(t0)
    mv a0, s0
    li a7, 57                 /* close last reader */
    ecall
    bnez a0, failed
    mv a0, s1
    la a1, byte_x
    li a2, 1
    li a7, 64                 /* no readers -> EPIPE */
    ecall
    li t0, -32
    bne a0, t0, failed
    mv a0, s1
    li a7, 57
    ecall
    bnez a0, failed

    li a0, 1
    la a1, success
    li a2, 19
    li a7, 64
    ecall
    li a0, 0
    j exit
failed:
    li a0, 1
exit:
    li a7, 93
    ecall
    j .
.section .rodata
byte_x:
    .byte 'x'
success:
    .ascii "M030_PIPE2_TEST_OK\n"
.section .bss
.balign 4
fds:
    .space 8
byte:
    .space 1

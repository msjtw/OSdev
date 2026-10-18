/* M-031: ppoll must not report empty pipes ready, and must report data/EOF. */
.section .text
.global _start
_start:
    la a0, fds
    li a1, 0
    li a7, 59                 /* pipe2 */
    ecall
    bnez a0, failed
    la t0, fds
    lw s0, 0(t0)
    lw s1, 4(t0)
    la t0, pollfd
    sw s0, 0(t0)

    la a0, pollfd
    li a1, 1
    la a2, zero_timeout
    li a3, 0
    li a4, 0
    li a7, 414                /* ppoll: empty pipe, zero timeout */
    ecall
    bnez a0, failed
    la t0, pollfd
    lhu t1, 6(t0)
    bnez t1, failed

    la a0, pollfd
    li a1, 1
    la a2, short_timeout
    li a3, 0
    li a4, 0
    li a7, 414                /* must sleep until timer wakes it */
    ecall
    bnez a0, failed

    mv a0, s1
    la a1, byte_x
    li a2, 1
    li a7, 64
    ecall
    li t0, 1
    bne a0, t0, failed

    la a0, pollfd
    li a1, 1
    la a2, zero_timeout
    li a3, 0
    li a4, 0
    li a7, 414                /* buffered byte is readable */
    ecall
    li t0, 1
    bne a0, t0, failed
    la t0, pollfd
    lhu t1, 6(t0)
    andi t1, t1, 1
    beqz t1, failed

    mv a0, s1
    li a7, 57                 /* close writer */
    ecall
    bnez a0, failed
    mv a0, s0
    la a1, byte
    li a2, 1
    li a7, 63                 /* consume byte */
    ecall
    li t0, 1
    bne a0, t0, failed

    la a0, pollfd
    li a1, 1
    la a2, zero_timeout
    li a3, 0
    li a4, 0
    li a7, 414                /* EOF is readable */
    ecall
    li t0, 1
    bne a0, t0, failed

    li a0, -100
    la a1, lorem
    li a2, 0
    li a3, 0
    li a7, 56                 /* openat regular ext4 file */
    ecall
    bltz a0, failed
    la t0, pollfd
    sw a0, 0(t0)
    la a0, pollfd
    li a1, 1
    la a2, zero_timeout
    li a3, 0
    li a4, 0
    li a7, 414                /* regular files are immediately readable */
    ecall
    li t0, 1
    bne a0, t0, failed
    la t0, pollfd
    lhu t1, 6(t0)
    andi t1, t1, 1
    beqz t1, failed

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
lorem:
    .asciz "/lorem.txt"
success:
    .ascii "M031_PPOLL_TEST_OK\n"
.section .data
.balign 4
zero_timeout:
    .word 0, 0, 0, 0
short_timeout:
    .word 0, 0, 10000000, 0
pollfd:
    .word 0
    .half 1                  /* POLLIN */
    .half 0
.section .bss
.balign 4
fds:
    .space 8
byte:
    .space 1

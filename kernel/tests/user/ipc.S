    .equ IPC_CALL, 0x80000001
    .equ IPC_RECEIVE, 0x80000002
    .equ IPC_REPLY, 0x80000004

    .section .text
    .globl _start
_start:
    li a7, 172
    ecall
    mv s0, a0                 /* Parent PID; child inherits this register. */
    li a0, 17                 /* clone(SIGCHLD) */
    li a1, 0
    li a2, 0
    li a3, 0
    li a4, 0
    li a7, 220
    ecall
    bltz a0, failed
    beqz a0, server
    mv s1, a0

#if CASE == 0
    mv a0, s1
    li a1, 0                  /* Invalid message pointer. */
    li a7, IPC_CALL
    ecall
    li t0, -14
    bne a0, t0, failed
    li s3, 0
call_loop:
    mv a0, s1
    la a1, message
    li a7, IPC_CALL
    ecall
    addi t0, s3, 100
    li t1, 31
    bne s3, t1, check_result
    li t0, -22                /* Negative reply result passes through unchanged. */
check_result:
    bne a0, t0, failed
    addi s3, s3, 1
    li t0, 32
    blt s3, t0, call_loop
#else
    li a0, 1                  /* Give the child time to block. */
    li a1, 0
    la a2, delay
    li a3, 0
    li a7, 407
    ecall
    bnez a0, failed
    mv a0, s1
    li a1, 15                 /* A signal must end the blocked IPC operation. */
    li a7, 129
    ecall
    bnez a0, failed
#endif

    li a0, 0                  /* Supported waitid(P_ALL, ..., WEXITED) subset. */
    li a1, 0
    la a2, siginfo
    li a3, 4
    li a4, 0
    li a7, 95
    ecall
    bnez a0, failed
    la t0, siginfo
    lw t1, 8(t0)
#if CASE == 0
    li t2, 1                  /* CLD_EXITED */
    bne t1, t2, failed
    lw t1, 20(t0)
    bnez t1, failed
#else
    li t2, 2                  /* CLD_KILLED */
    bne t1, t2, failed
    lw t1, 20(t0)
    li t2, 15
    bne t1, t2, failed
#endif
    li a0, 1
    la a1, success
    li a2, success_end-success
    li a7, 64
    ecall
    li a0, 0
    li a7, 93
    ecall

server:
#if CASE == 1
    li a0, 999999             /* Missing destination drops the message, not the wait. */
    la a1, message
    li a7, IPC_CALL
    ecall
    j failed
#elif CASE == 2
    la a0, received
    li a7, IPC_RECEIVE
    ecall
    j failed
#else
    li a0, 0                  /* EFAULT must leave the oldest message queued. */
    li a7, IPC_RECEIVE
    ecall
    li t0, -14
    bne a0, t0, failed
    li s3, 0
receive_loop:
    la a0, received
    li a7, IPC_RECEIVE
    ecall
    blez a0, failed
    mv s2, a0
    la t0, received
    lw t1, 0(t0)
    bne t1, s0, failed        /* Kernel overwrote the forged sender. */
    lw t1, 4(t0)
    li t2, 0x80000040
    bne t1, t2, failed
    addi t0, t0, 8
    la t1, message
    addi t1, t1, 8
    li t2, 6
check_args:
    lw t3, 0(t0)
    lw t4, 0(t1)
    bne t3, t4, failed
    addi t0, t0, 4
    addi t1, t1, 4
    addi t2, t2, -1
    bnez t2, check_args

    bnez s3, make_reply
    mv a0, s2
    li a1, 0
    li a2, 0
    li a3, 1                  /* Invalid payload source must not consume the request. */
    li a7, IPC_REPLY
    ecall
    li t0, -14
    bne a0, t0, failed
make_reply:
    mv a0, s2
    addi a1, s3, 100
    li t0, 31
    bne s3, t0, reply_now
    li a1, -22
reply_now:
    li a2, 0
    li a3, 0
    li a7, IPC_REPLY
    ecall
    bnez a0, failed
    mv a0, s2                 /* Duplicate reply must not complete a newer call. */
    li a1, 999
    li a2, 0
    li a3, 0
    li a7, IPC_REPLY
    ecall
    bnez a0, failed
    addi s3, s3, 1
    li t0, 32
    blt s3, t0, receive_loop
    li a0, 0
    li a7, 93
    ecall
#endif

failed:
    li a0, 1
    li a7, 93
    ecall
    j failed

    .section .rodata
success:
    .ascii "IPC_TRANSPORT_OK\n"
success_end:
    .balign 4
message:
    .word 777, 0x80000040, 11, 22, 33, 44, 55, 66
    .balign 8
delay:
    .quad 0, 200000000

    .section .bss
    .balign 4
received:
    .space 32
siginfo:
    .space 128

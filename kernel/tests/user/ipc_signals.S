    .equ IPC_CALL, 0x80000001
    .equ IPC_RECEIVE, 0x80000002
    .equ IPC_REPLY, 0x80000004

    .section .text
    .globl _start
_start:
#if CASE == 1
    la a0, pipefds
    li a1, 0
    li a7, 59
    ecall
    bnez a0, failed
#endif
    li a7, 172
    ecall
    mv s0, a0                 /* Parent is the server and can signal its child. */
    li a0, 17
    li a1, 0
    li a2, 0
    li a3, 0
    li a4, 0
    li a7, 220
    ecall
    bltz a0, failed
    beqz a0, client
    mv s1, a0

#if CASE == 1 || CASE == 2
    /* Let the child queue A, or block in receive, before sending the signal. */
    jal ra, short_delay
    li a1, 10
    jal ra, signal_child
#if CASE == 1
    /* Wait for acknowledgement that A returned EINTR and was invalidated. */
    la t0, pipefds
    lw a0, 0(t0)
    la a1, ack
    li a2, 1
    li a7, 63
    ecall
    li t0, 1
    bne a0, t0, failed
#endif
#else
    la a0, incoming
    li a7, IPC_RECEIVE
    ecall
    blez a0, failed
    mv s2, a0
    la t0, incoming
    lw t1, 0(t0)
    bne t1, s1, failed
    lw t1, 4(t0)
    li t2, 0x80000101
    bne t1, t2, failed
#if CASE == 5
    li a1, 17                 /* Default-ignored SIGCHLD must not interrupt IPC. */
#elif CASE == 6
    li a1, 15
#else
    li a1, 10
#endif
    jal ra, signal_child
#if CASE >= 3 && CASE <= 5
    /* Run the client after signal delivery, before making the reply available. */
    jal ra, short_delay
    mv a0, s2
    li a1, 111
    li a2, 0
    li a3, 0
    li a7, IPC_REPLY
    ecall
    bnez a0, failed
#endif
#endif

#if CASE != 6
    /* Receiving B proves the first wait ended and the caller started a new one. */
    la a0, incoming
    li a7, IPC_RECEIVE
    ecall
    blez a0, failed
    mv s3, a0
#if CASE == 0 || CASE >= 3
    beq s2, s3, failed
#endif
    la t0, incoming
    lw t1, 0(t0)
    bne t1, s1, failed
    lw t1, 4(t0)
    li t2, 0x80000102
    bne t1, t2, failed        /* A queued cancellation must not reach receive. */
#if CASE == 0
    /* Old replies, with and without a payload, cannot complete B. */
    mv a0, s2
    li a1, 999
    li a2, 0
    li a3, 0
    li a7, IPC_REPLY
    ecall
    bnez a0, failed
    mv a0, s2
    li a1, 999
    li a2, -1
    li a3, 1
    li a7, IPC_REPLY
    ecall
    bnez a0, failed
    mv a0, s2
    li a1, 0
    li a2, -1
    li a3, 1
    li a7, 0x80000003         /* A cancelled request cannot read caller memory. */
    ecall
    li t0, -3
    bne a0, t0, failed
#endif
    mv a0, s3
    li a1, 222
    li a2, 0
    li a3, 0
    li a7, IPC_REPLY
    ecall
    bnez a0, failed
#endif

    li a0, 0                  /* Supported waitid(P_ALL, ..., WEXITED). */
    li a1, 0
    la a2, siginfo
    li a3, 4
    li a4, 0
    li a7, 95
    ecall
    bnez a0, failed
    la t0, siginfo
    lw t1, 8(t0)
#if CASE == 6
    li t2, 2
    bne t1, t2, failed
    lw t1, 20(t0)
    li t2, 15
    bne t1, t2, failed
    /* The dead caller's delivered request must no longer accept reply data. */
    mv a0, s2
    li a1, 999
    li a2, -1
    li a3, 1
    li a7, IPC_REPLY
    ecall
    bnez a0, failed
#else
    li t2, 1
    bne t1, t2, failed
    lw t1, 20(t0)
    bnez t1, failed
#endif
    li a0, 1
    la a1, success
    li a2, success_end-success
    li a7, 64
    ecall
    li a0, 0
    li a7, 93
    ecall

client:
    li a0, 10
#if CASE == 4
    la a1, ignored_action
#else
    la a1, action
#endif
    li a2, 0
    li a3, 8
    li a7, 134
    ecall
    bnez a0, failed
#if CASE == 3
    li a0, 0                  /* Block SIGUSR1 while waiting for A. */
    la a1, usr1_mask
    li a2, 0
    li a3, 8
    li a7, 135
    ecall
    bnez a0, failed
#endif
#if CASE == 2
    la a0, incoming
    li a7, IPC_RECEIVE
    ecall
#else
    mv a0, s0
    la a1, message_a
    li a7, IPC_CALL
    ecall
#endif
#if CASE == 6
    j failed                  /* SIGTERM must terminate, not return to userspace. */
#elif CASE <= 2
    li t0, -4
    bne a0, t0, failed        /* rt_sigreturn restores the EINTR syscall result. */
    la t0, handled
    lw t1, 0(t0)
    li t2, 1
    bne t1, t2, failed
#else
    li t0, 111
    bne a0, t0, failed
    la t0, handled
    lw t1, 0(t0)
    bnez t1, failed
#if CASE == 3
    li a0, 1                  /* Unblock: deferred SIGUSR1 now runs its handler. */
    la a1, usr1_mask
    li a2, 0
    li a3, 8
    li a7, 135
    ecall
    bnez a0, failed
    la t0, handled
    lw t1, 0(t0)
    li t2, 1
    bne t1, t2, failed
#endif
#endif
#if CASE == 1
    la t0, pipefds
    lw a0, 4(t0)
    la a1, ack
    li a2, 1
    li a7, 64
    ecall
    li t0, 1
    bne a0, t0, failed
#endif
    mv a0, s0
    la a1, message_b
    li a7, IPC_CALL
    ecall
    li t0, 222
    bne a0, t0, failed
    li a0, 0
    li a7, 93
    ecall

handler:
    li t0, 10
    bne a0, t0, failed
    la t0, handled
    lw t1, 0(t0)
    addi t1, t1, 1
    sw t1, 0(t0)
    ret

restorer:
    li a7, 139
    ecall
    j failed

signal_child:
    mv a0, s1
    li a7, 129
    ecall
    bnez a0, failed
    ret

short_delay:
    li a0, 1
    li a1, 0
    la a2, delay
    li a3, 0
    li a7, 407
    ecall
    bnez a0, failed
    ret

failed:
    li a0, 1
    li a7, 93
    ecall
    j failed

    .section .rodata
    .balign 4
action:
    .word handler, 0, restorer, 0, 0
ignored_action:
    .word 1, 0, 0, 0, 0
usr1_mask:
    .word 512, 0
message_a:
    .word 777, 0x80000101, 0, 0, 0, 0, 0, 0
message_b:
    .word 777, 0x80000102, 0, 0, 0, 0, 0, 0
    .balign 8
delay:
    .quad 0, 200000000
success:
    .ascii "IPC_SIGNALS_OK\n"
success_end:

    .section .bss
    .balign 4
handled:
    .space 4
incoming:
    .space 32
siginfo:
    .space 128
pipefds:
    .space 8
ack:
    .space 1

    .section .text
    .globl _start
_start:
    li a0, 0                  /* SIG_BLOCK */
    la a1, sigint_mask
    li a2, 0
    li a3, 8
    li a7, 135                /* rt_sigprocmask */
    ecall
    bnez a0, failed
    li a0, 2                  /* SIG_SETMASK */
    la a1, empty_mask
    la a2, previous_mask
    li a3, 8
    li a7, 135
    ecall
    bnez a0, failed
    la t0, previous_mask
    lw t1, 0(t0)
    li t2, 2                  /* SIGINT was blocked, then restored */
    bne t1, t2, failed

    li a0, 2                  /* SIGINT */
    la a1, action
    li a2, 0
    li a3, 8                  /* sizeof(sigset_t) */
    li a7, 134                /* rt_sigaction */
    ecall
    bnez a0, failed

    li a0, 1
    la a1, ready
    li a2, ready_end-ready
    li a7, 64                 /* write */
    ecall

    li a0, 0
    la a1, byte
    li a2, 1
    li a7, 63                 /* read: waits for terminal SIGINT */
    ecall
    li t0, -4                 /* EINTR after the handler returned */
    bne a0, t0, failed

    li a0, 1
    la a1, success
    li a2, success_end-success
    li a7, 64
    ecall
    li a0, 0
    li a7, 93
    ecall

handler:
    li t0, 2
    bne a0, t0, failed
    li a0, 1
    la a1, handled
    li a2, handled_end-handled
    li a7, 64
    ecall
    ret

restorer:
    li a7, 139                /* rt_sigreturn */
    ecall
    j failed                  /* sigreturn must not return here */

failed:
    li a0, 1
    li a7, 93
    ecall

    .section .rodata
    .align 2
action:
    .word handler
    .word 0
    .word restorer
    .word 0
    .word 0
sigint_mask:
    .word 2
    .word 0
empty_mask:
    .word 0
    .word 0
ready:
    .ascii "M033_READY\n"
ready_end:
handled:
    .ascii "M033_HANDLER_OK\n"
handled_end:
success:
    .ascii "M033_SIGNAL_TEST_OK\n"
success_end:
    .section .bss
    .align 2
byte:
    .space 1
    .align 2
previous_mask:
    .space 8

    .section .text
    .globl _start
_start:
    li a7, 172                /* getpid */
    ecall
    mv s0, a0
    mv a0, s0
    li a1, 0                  /* kill(self, 0) */
    li a7, 129
    ecall
    bnez a0, failed

    li a0, -1                 /* broadcast is intentionally unsupported */
    li a1, 0
    li a7, 129
    ecall
    li t0, -22
    bne a0, t0, failed
    li a0, 9999               /* nonexistent PID */
    li a1, 0
    li a7, 129
    ecall
    li t0, -3
    bne a0, t0, failed

    li a7, 173                /* getppid: parents are outside kill scope */
    ecall
    li a1, 0
    li a7, 129
    ecall
    li t0, -1                 /* EPERM */
    bne a0, t0, failed

    li a0, 0                  /* Probe our group; it includes this process. */
    li a1, 0
    li a7, 129
    ecall
    bnez a0, failed

    li a0, 17                 /* clone(SIGCHLD) */
    li a1, 0
    li a2, 0
    li a3, 0
    li a4, 0
    li a7, 220
    ecall
    bltz a0, failed
    beqz a0, child
    mv s1, a0
    li a0, 1                  /* Let the child create its group and sleep. */
    li a1, 0
    la a2, parent_delay
    li a3, 0
    li a7, 407                /* clock_nanosleep_time64 */
    ecall
    bnez a0, failed
    sub a0, zero, s1          /* kill(-child_pid, SIGTERM) */
    li a1, 15                 /* SIGTERM direct child */
    li a7, 129
    ecall
    bnez a0, failed

    li a0, 0                  /* waitid(P_ALL, ..., WEXITED) */
    li a1, 0
    la a2, siginfo
    li a3, 4
    li a4, 0
    li a7, 95
    ecall
    bnez a0, failed
    la t0, siginfo
    lw t1, 8(t0)              /* CLD_KILLED */
    li t2, 2
    bne t1, t2, failed
    lw t1, 20(t0)             /* terminating SIGTERM */
    li t2, 15
    bne t1, t2, failed
    li a0, 1
    la a1, success
    li a2, success_end-success
    li a7, 64
    ecall
    li a0, 0
    li a7, 93
    ecall

child:
    li a0, 0                  /* Become leader of a child-only group. */
    li a1, 0
    li a7, 154                /* setpgid(0, 0) */
    ecall
    bnez a0, failed
    li a0, 1                  /* Block until the parent signals our group. */
    li a1, 0
    la a2, child_delay
    li a3, 0
    li a7, 407                /* clock_nanosleep_time64 */
    ecall
    j failed                  /* SIGTERM must take the default action. */

failed:
    li a0, 1
    li a7, 93
    ecall

    .section .rodata
success:
    .ascii "M034_KILL_TEST_OK\n"
success_end:
    .align 3
parent_delay:
    .dword 0
    .dword 100000000
child_delay:
    .dword 5
    .dword 0
    .section .bss
    .align 2
siginfo:
    .space 128

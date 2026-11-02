/* IPC-011 smoke: execute the real UART ELF without any test-only MMIO grant. */
typedef unsigned long word;
#ifndef UART_ADDRESS_ARG
#define UART_ADDRESS_ARG "0x10000000"
#endif
struct Message { word sender, operation, args[6]; };
static word siginfo[32];

static word sc(word number, word x0, word x1, word x2, word x3, word x4) {
    register word a0 __asm__("a0") = x0;
    register word a1 __asm__("a1") = x1;
    register word a2 __asm__("a2") = x2;
    register word a3 __asm__("a3") = x3;
    register word a4 __asm__("a4") = x4;
    register word a5 __asm__("a5") = 0;
    register word a7 __asm__("a7") = number;
    __asm__ volatile("ecall" : "+r"(a0) : "r"(a1), "r"(a2), "r"(a3), "r"(a4), "r"(a5), "r"(a7) : "memory");
    return a0;
}

static __attribute__((noreturn)) void leave(word status) {
    sc(93, status, 0, 0, 0, 0);
    for (;;) {}
}

static void check(int condition) {
    if (!condition) leave(1);
}

__attribute__((noreturn)) void _start(void) {
    word child = sc(220, 17, 0, 0, 0, 0);
    check((long)child >= 0);
    if (child == 0) {
        static const char path[] = "/sbin/uart";
        static const char address[] = UART_ADDRESS_ARG;
        const char *argv[] = {path, address, (const char *)0};
        sc(221, (word)path, (word)argv, 0, 0, 0);
        leave(1);
    }
    const word operations[] = {63, 64, 0x8000ffffUL};
    for (word i = 0; i < 3; ++i) {
        /* Claimed kernel sender is overwritten; no request may touch MMIO. */
        struct Message message = {0, operations[i], {0, 0, 8197, 0, 0, 0}};
        check(sc(0x80000001UL, child, (word)&message, 0, 0, 0) == (word)-1);
    }
    check(sc(129, child, 0, 0, 0, 0) == 0);
    check(sc(129, child, 15, 0, 0, 0) == 0);
    for (;;) {
        word result = sc(95, 0, 0, (word)siginfo, 4, 0);
        if (result == (word)-4) continue;
        check(result == 0);
        break;
    }
    check(siginfo[2] == 2 && siginfo[3] == child && siginfo[5] == 15);
    static const char success[] = "UART_SERVER_OK\n";
    sc(64, 1, (word)success, sizeof(success) - 1, 0, 0);
    leave(0);
}

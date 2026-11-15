/* Real TTY write IPC: completion counts, server OOM, writev, and recovery. */
typedef unsigned long word;
struct Iovec { word base, length; };
static unsigned char bytes[131072];

static word sc(word number, word x0, word x1, word x2) {
    register word a0 __asm__("a0") = x0;
    register word a1 __asm__("a1") = x1;
    register word a2 __asm__("a2") = x2;
    register word a7 __asm__("a7") = number;
    __asm__ volatile("ecall" : "+r"(a0) : "r"(a1), "r"(a2), "r"(a7) : "memory");
    return a0;
}

static void check(int condition) {
    if (!condition) {
        sc(93, 1, 0, 0);
        for (;;) {}
    }
}

__attribute__((noreturn)) void _start(void) {
    check(sc(64, 1, (word)bytes, sizeof(bytes)) == (word)-12);
    check(sc(64, 1, (word)-1, 0) == 0);
    check(sc(64, 1, (word)-1, 16) == (word)-14);
    const char first[] = "IPC021_WRITEV_OK\n";
    struct Iovec vectors[] = {{(word)first, sizeof(first) - 1}, {(word)bytes, sizeof(bytes)}};
    check(sc(66, 1, (word)vectors, 2) == sizeof(first) - 1);
    for (word i = 0; i < 8192; ++i) bytes[i] = 'w';
    check(sc(64, 1, (word)bytes, 8192) == 8192);
    const char done[] = "\nIPC021_WRITE_OK\n";
    check(sc(64, 1, (word)done, sizeof(done) - 1) == sizeof(done) - 1);
    sc(93, 0, 0, 0);
    for (;;) {}
}

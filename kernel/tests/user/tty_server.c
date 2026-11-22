/* Ordinary clients of the real TTY executable, without production test hooks. */
typedef unsigned long word;
struct Message { word sender, operation, args[6]; };
struct Event { word kind, id, result; struct Message message; };
struct Termios { unsigned int iflag, oflag, cflag, lflag; unsigned char line, cc[19]; };
static word siginfo[32];
static word tty_ioctl(word child, word operation, void *buffer, word size);

static word sc(word number, word x0, word x1, word x2, word x3, word x4) {
    register word a0 __asm__("a0") = x0;
    register word a1 __asm__("a1") = x1;
    register word a2 __asm__("a2") = x2;
    register word a3 __asm__("a3") = x3;
    register word a4 __asm__("a4") = x4;
    register word a5 __asm__("a5") = 0;
    register word a7 __asm__("a7") = number;
    __asm__ volatile("ecall" : "+r"(a0) : "r"(a1), "r"(a2), "r"(a3),
                     "r"(a4), "r"(a5), "r"(a7) : "memory");
    return a0;
}

static __attribute__((noreturn)) void leave(word status) {
    sc(93, status, 0, 0, 0, 0);
    for (;;) {}
}

static void check(int condition) {
    if (!condition) leave(1);
}

static word tty_ioctl(word child, word operation, void *buffer, word size) {
    struct Message message = {0, 0x80000200UL, {0, (word)buffer, size, operation, 0, 0}};
    return sc(0x80000001UL, child, (word)&message, 0, 0, 0);
}

__attribute__((noreturn)) void _start(void) {
    /* Discover the actual embedded UART PID without assuming its numeric value. */
    word self = sc(172, 0, 0, 0, 0, 0), uart = 0, embedded_tty = 0;
    for (word pid = 2; pid < self; ++pid) {
        if (sc(129, pid, 0, 0, 0, 0) == (word)-1) {
            struct Message probe = {0, 64, {0}};
            word result = sc(0x80000001UL, pid, (word)&probe, 0, 0, 0);
            if (result == (word)-1) {
                check(uart == 0);
                uart = pid;
            } else {
                check(result == 0 && embedded_tty == 0);
                embedded_tty = pid;
            }
        }
    }
    check(uart != 0);
    check(embedded_tty != 0);
    for (word operation = 0x80000100UL; operation <= 0x80000103UL; ++operation)
        check(sc(operation, 0, 0, 0, 0, 0) == (word)-1);
#ifdef TTY_BOOTSTRAP
    word child = embedded_tty;
#else
    char text[16];
    word position = sizeof(text) - 1;
    text[position] = 0;
    do { text[--position] = '0' + uart % 10; uart /= 10; } while (uart);

    word child = sc(220, 17, 0, 0, 0, 0);
    check((long)child >= 0);
    if (child == 0) {
        static const char path[] = "/tty-driver";
        const char *argv[] = {path, text + position, (const char *)0};
        sc(221, (word)path, (word)argv, 0, 0, 0);
        leave(1);
    }
#endif
    /* No readiness handshake: IPC queued before exec is served by the real loop. */
    /* Unfinished reads must not prevent this same client from querying settings. */
    unsigned char input[32];
    struct Message read = {0, 63, {0, (word)input, sizeof(input), 0, 0, 0}};
    word pending[2];
    for (word i = 0; i < 2; ++i) {
        pending[i] = sc(0x80000005UL, child, (word)&read, 0, 0, 0);
        check((long)pending[i] > 0);
    }
    const word operations[] = {0x7ffeUL, 29, 0x8000ffffUL, 0x7fffUL};
    for (word i = 0; i < 4; ++i) {
        struct Message message = {0, operations[i], {0, 0xffffffffUL, 8197, 0, 0, 0}};
        check(sc(0x80000001UL, child, (word)&message, 0, 0, 0) == (word)-38);
    }
    word ids[4], seen = 0;
    for (word i = 0; i < 4; ++i) {
        struct Message message = {0, operations[i], {0, 0, 1, 0, 0, 0}};
        ids[i] = sc(0x80000005UL, child, (word)&message, 0, 0, 0);
        check((long)ids[i] > 0);
        for (word j = 0; j < i; ++j) check(ids[j] != ids[i]);
    }
    for (word i = 0; i < 4; ++i) {
        struct Event event;
        check(sc(0x80000006UL, (word)&event, 0, 0, 0, 0) == 0);
        check(event.kind == 1 && event.result == (word)-38);
        word j;
        for (j = 0; j < 4 && ids[j] != event.id; ++j) {}
        check(j < 4 && !(seen & (1UL << j)));
        seen |= 1UL << j;
    }
    check(seen == 15);
    struct Message invalid_write = {0, 64, {0, 0xffffffffUL, 1, 0, 0, 0}};
    check(sc(0x80000001UL, child, (word)&invalid_write, 0, 0, 0) == (word)-14);
#ifdef TTY_BOOTSTRAP
    static const char output[] = "TTY_UART_WRITE_OK\n";
    struct Message write = {0, 64, {0, (word)output, sizeof(output) - 1, 0, 0, 0}};
    word writing = sc(0x80000005UL, child, (word)&write, 0, 0, 0);
    check((long)writing > 0);
    unsigned int foreground;
    check(tty_ioctl(child, 0x540f, &foreground, sizeof(foreground)) == 0);
    struct Event completion;
    check(sc(0x80000006UL, (word)&completion, 0, 0, 0, 0) == 0);
    check(completion.kind == 1 && completion.id == writing && completion.result == sizeof(output) - 1);
#endif
    struct Message last = {0, 64, {0}};
    check(sc(0x80000001UL, child, (word)&last, 0, 0, 0) == 0);
    last.operation = 63;
    last.args[1] = 0xffffffffUL;
    check(sc(0x80000001UL, child, (word)&last, 0, 0, 0) == 0);
    struct Termios settings, returned;
    check(sizeof(settings) == 36);
    check(tty_ioctl(child, 0x5401, &settings, sizeof(settings)) == 0);
    check(settings.iflag == 0400 && settings.lflag == 013 && settings.cc[0] == 3);
    unsigned char original_settings[36];
    for (word i = 0; i < 36; ++i)
        original_settings[i] = ((unsigned char *)&settings)[i];
    settings.lflag = 0;
    settings.iflag = 0;
    settings.cflag = 0x12345678;
    settings.cc[0] = '!';
    check(tty_ioctl(child, 0x5402, &settings, sizeof(settings)) == 0);
    check(tty_ioctl(child, 0x5402, &settings, 35) == (word)-22);
    check(tty_ioctl(child, 0x5402, (void *)0xffffffffUL, 36) == (word)-14);
    check(tty_ioctl(child, 0x5401, &returned, 36) == 0);
    const unsigned char *a = (const unsigned char *)&settings, *b = (const unsigned char *)&returned;
    for (word i = 0; i < 36; ++i) check(a[i] == b[i]);
    check(tty_ioctl(child, 0x5401, (void *)0xffffffffUL, 36) == (word)-14);
    check(tty_ioctl(child, 0x5401, &returned, 35) == (word)-22);
    unsigned short size[4];
    check(tty_ioctl(child, 0x5413, size, sizeof(size)) == 0);
    check(size[0] == 24 && size[1] == 80 && !size[2] && !size[3]);
    unsigned int pgid = 123;
    check(tty_ioctl(child, 0x540f, &pgid, sizeof(pgid)) == 0);
#ifdef TTY_BOOTSTRAP
    check(pgid != 0);
    word original = pgid;
    pgid = (unsigned int)sc(155, 0, 0, 0, 0, 0);
    check(tty_ioctl(child, 0x5410, &pgid, sizeof(pgid)) == 0);
    unsigned int valid = pgid;
    pgid = 0xffffffffUL;
    check(tty_ioctl(child, 0x5410, &pgid, sizeof(pgid)) == (word)-1);
    check(tty_ioctl(child, 0x540f, &pgid, sizeof(pgid)) == 0 && pgid == valid);
    pgid = original;
    check(tty_ioctl(child, 0x5410, &pgid, sizeof(pgid)) == 0);
    word outsider = sc(220, 17, 0, 0, 0, 0);
    check((long)outsider >= 0);
    if (outsider == 0) {
        check((long)sc(157, 0, 0, 0, 0, 0) > 0);
        check(tty_ioctl(child, 0x5410, &pgid, sizeof(pgid)) == (word)-1);
        leave(0);
    }
    for (;;) {
        word result = sc(95, 0, 0, (word)siginfo, 4, 0);
        if (result == (word)-4) continue;
        check(result == 0);
        break;
    }
    check(siginfo[3] == outsider && siginfo[5] == 0);
#else
    check(pgid == 0);
    check(tty_ioctl(child, 0x5410, &pgid, sizeof(pgid)) == (word)-1);
#endif
    pgid = 0;
    check(tty_ioctl(child, 0x5410, &pgid, sizeof(pgid)) == (word)-1);
    check(tty_ioctl(child, 0x5410, &pgid, 3) == (word)-22);
    check(tty_ioctl(child, 0x5410, (void *)0xffffffffUL, 4) == (word)-14);
    check(tty_ioctl(child, 0xffff, 0, 0) == (word)-25);
    for (word i = 0; i < 2; ++i) {
        /* Discard ownership after checking responsiveness with outstanding reads. */
        check(sc(0x80000007UL, pending[i], 0, 0, 0, 0) == 0);
    }
    word reader = sc(220, 17, 0, 0, 0, 0);
    check((long)reader >= 0);
    if (reader == 0) {
        check((long)sc(0x80000005UL, child, (word)&read, 0, 0, 0) > 0);
        leave(0);
    }
    check(sc(95, 0, 0, (word)siginfo, 4, 0) == 0);
    check(siginfo[3] == reader && siginfo[5] == 0);
    check(tty_ioctl(child, 0x5401, &returned, 36) == 0);
#ifdef TTY_BOOTSTRAP
    /* The embedded server now owns the shell's real terminal settings. */
    check(tty_ioctl(child, 0x5402, original_settings, 36) == 0);
#endif
#ifndef TTY_BOOTSTRAP
    check(sc(129, child, 15, 0, 0, 0) == 0);
    for (;;) {
        word result = sc(95, 0, 0, (word)siginfo, 4, 0);
        if (result == (word)-4) continue;
        check(result == 0);
        break;
    }
    check(siginfo[2] == 2 && siginfo[3] == child && siginfo[5] == 15);
#endif
    static const char success[] = "TTY_SERVER_OK\n";
    sc(64, 1, (word)success, sizeof(success) - 1, 0, 0);
    leave(0);
}

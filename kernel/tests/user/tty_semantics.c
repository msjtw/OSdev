/* IPC-022: ordinary Linux TTY ioctls, readiness, and blocked-reader wakeups. */
typedef unsigned long word;
struct Termios { unsigned int iflag, oflag, cflag, lflag; unsigned char line, cc[19]; };
struct PollFd { int fd; short events, revents; };
struct Timespec { long long sec, nsec; };
static struct Termios saved, mode;
static char bytes[16];
static word tty_fd;

static word sc(word nr, word x0, word x1, word x2, word x3, word x4) {
    register word a0 __asm__("a0") = x0;
    register word a1 __asm__("a1") = x1;
    register word a2 __asm__("a2") = x2;
    register word a3 __asm__("a3") = x3;
    register word a4 __asm__("a4") = x4;
    register word a5 __asm__("a5") = 0;
    register word a7 __asm__("a7") = nr;
    __asm__ volatile("ecall" : "+r"(a0) : "r"(a1), "r"(a2), "r"(a3), "r"(a4), "r"(a5), "r"(a7) : "memory");
    return a0;
}

static __attribute__((noreturn)) void leave(word status) {
    sc(93, status, 0, 0, 0, 0);
    for (;;) {}
}
#define STRINGIFY_(x) #x
#define STRINGIFY(x) STRINGIFY_(x)
#define check(ok) do { if (!(ok)) { \
    static const char failure[] = "IPC022_FAIL:" STRINGIFY(__LINE__) "\n"; \
    sc(64, 1, (word)failure, sizeof(failure) - 1, 0, 0); leave(1); \
} } while (0)
#define SAY(text) check(sc(64, 1, (word)(text), sizeof(text) - 1, 0, 0) == sizeof(text) - 1)

static void set_mode(void) {
    check(sc(29, 0, 0x5402, (word)&mode, 0, 0) == 0);
    struct Termios actual;
    check(sc(29, 0, 0x5401, (word)&actual, 0, 0) == 0);
    check(actual.iflag == mode.iflag && actual.lflag == mode.lflag);
}

static word poll_tty(short events, word milliseconds, short expected) {
    struct PollFd fd = {(int)tty_fd, events, -1};
    struct Timespec timeout = {milliseconds / 1000, (milliseconds % 1000) * 1000000};
    word result = sc(414, (word)&fd, 1, (word)&timeout, 0, 0);
    check(fd.revents == expected);
    return result;
}

static void read_exact(word length) {
    word count = 0;
    while (count < length) {
        word got = sc(63, 0, (word)bytes + count, length - count, 0, 0);
        check(got > 0 && got <= length - count);
        count += got;
    }
}

__attribute__((noreturn)) void _start(void) {
    check(sc(29, 0, 0x5401, (word)&saved, 0, 0) == 0);
    check(sc(29, 0, 0x5401, (word)&mode, 0, 0) == 0);
    word foreground = 0;
    check(sc(29, 0, 0x540f, (word)&foreground, 0, 0) == 0);
    check(foreground == sc(155, 0, 0, 0, 0, 0));
    tty_fd = sc(56, (word)-100, (word)"/dev/tty", 2, 0, 0);
    check((long)tty_fd >= 0);
    mode.iflag |= 0400;  /* ICRNL */
    mode.lflag = (mode.lflag | 3) & ~010; /* ISIG | ICANON, no ECHO */
    set_mode();

#if CASE == 4
    int pipefd[2];
    check(sc(59, (word)pipefd, 0, 0, 0, 0) == 0);
    word child = sc(220, 17, 0, 0, 0, 0);
    check((long)child >= 0);
    if (child == 0) {
        check(sc(57, pipefd[0], 0, 0, 0, 0) == 0);
        char ready = 'R';
        check(sc(64, pipefd[1], (word)&ready, 1, 0, 0) == 1);
        read_exact(9);
        const char expected[] = "parallel\n";
        for (word i = 0; i < 9; ++i) check(bytes[i] == expected[i]);
        ready = 'D';
        check(sc(64, pipefd[1], (word)&ready, 1, 0, 0) == 1);
        leave(0);
    }
    check(sc(57, pipefd[1], 0, 0, 0, 0) == 0);
    char status;
    check(sc(63, pipefd[0], (word)&status, 1, 0, 0) == 1 && status == 'R');
    /* Allow the child to enter read; do not send UART input until parent I/O
       and settings operations have completed. The pipe must remain empty. */
    struct Timespec delay = {0, 300000000};
    check(sc(414, 0, 0, (word)&delay, 0, 0) == 0);
    struct PollFd pending = {pipefd[0], 1, -1};
    struct Timespec zero = {0, 0};
    check(sc(414, (word)&pending, 1, (word)&zero, 0, 0) == 0);
    mode.iflag &= ~0400; /* Change ICRNL while the other client is waiting. */
    set_mode();
    SAY("IPC030_CONCURRENT_WAIT\n");
    check(sc(63, pipefd[0], (word)&status, 1, 0, 0) == 1 && status == 'D');
    word info[32];
    check(sc(95, 0, 0, (word)info, 4, 0) == 0);
    check(info[3] == child && info[5] == 0);
    check(sc(57, pipefd[0], 0, 0, 0, 0) == 0);
#elif CASE == 1
    /* Writable is acceptance readiness, not a promise to avoid an IPC wait. */
    check(poll_tty(1 | 4, 0, 4) == 1);
    check(sc(63, 0, (word)-1, 0, 0, 0) == 0);
    SAY("IPC022_CANON_WAIT\n");
    check(poll_tty(1, 300, 0) == 0); /* Partial canonical input is not readable. */
    SAY("IPC022_PARTIAL_OK\n");
    check(poll_tty(1, 5000, 1) == 1);
    check(sc(63, 0, (word)bytes, 2, 0, 0) == 2 && bytes[0] == 'a' && bytes[1] == 'b');
    check(poll_tty(1 | 4, 0, 1 | 4) == 1);
    check(sc(63, 0, (word)bytes, sizeof(bytes), 0, 0) == 2 && bytes[0] == 'c' && bytes[1] == '\n');
    check(poll_tty(1, 0, 0) == 0);

    mode.iflag &= ~0400;
    mode.lflag &= ~(3 | 010); /* Noncanonical, no ISIG or ECHO. */
    set_mode();
    SAY("IPC022_RAW_WAIT\n");
    check(poll_tty(1, 5000, 1) == 1);
    read_exact(2);
    check(bytes[0] == 3 && bytes[1] == '\r');
    check(poll_tty(1 | 4, 0, 4) == 1);

    mode.iflag |= 0400;
    mode.lflag |= 3 | 010;
    set_mode();
    SAY("IPC022_ECHO_WAIT\n");
    read_exact(5);
    check(bytes[0] == 'l' && bytes[1] == 'o' && bytes[2] == 'u' && bytes[3] == 'd' && bytes[4] == '\n');
#else
    SAY("IPC022_MODE_WAIT\n");
    check(poll_tty(1, 300, 0) == 0);
    word child = sc(220, 17, 0, 0, 0, 0);
    check((long)child >= 0);
    if (child == 0) {
        struct Timespec delay = {0, 200000000};
        check(sc(407, 1, 0, (word)&delay, 0, 0) == 0);
        mode.lflag &= ~2; /* Buffered input becomes readable without a new byte. */
        set_mode();
        leave(0);
    }
#if CASE == 2
    check(poll_tty(1, 5000, 1) == 1);
#endif
    read_exact(4);
    check(bytes[0] == 'm' && bytes[1] == 'o' && bytes[2] == 'd' && bytes[3] == 'e');
    word info[32];
    check(sc(95, 0, 0, (word)info, 4, 0) == 0);
    check(info[3] == child && info[5] == 0);
#endif

    check(sc(29, 0, 0x5402, (word)&saved, 0, 0) == 0);
    check(sc(57, tty_fd, 0, 0, 0, 0) == 0);
    SAY("IPC022_TTY_OK\n");
    leave(0);
}

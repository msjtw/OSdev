/* Standalone RV32 IPC-005 client/server regression, with no libc. */
typedef unsigned long word;
typedef unsigned char byte;

enum { READ = 63, WRITE = 64 };
#define CALL 0x80000001UL
#define RECEIVE 0x80000002UL
#define PAYLOAD 0x80000003UL
#define REPLY 0x80000004UL
#define CUSTOM 0x80001234UL
#define LENGTH 8197UL

struct Message { word sender, operation, args[6]; };
static byte input[16384] __attribute__((aligned(4096)));
static byte output[16384] __attribute__((aligned(4096)));
static byte scratch[16384] __attribute__((aligned(4096)));
static struct Message incoming;
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

static void expect(word actual, long expected) {
    if (actual != (word)expected) {
        static const char failure[] = "IPC_PAYLOAD_FAILED\n";
        sc(WRITE, 1, (word)failure, sizeof(failure) - 1, 0, 0);
        leave(1);
    }
}

static word call(word server, word operation, word buffer, word length) {
    struct Message message = {777, operation, {1, buffer, length, 0, 0, 0}};
    return sc(CALL, server, (word)&message, 0, 0, 0);
}

static word receive(word client, word operation) {
    word id = sc(RECEIVE, (word)&incoming, 0, 0, 0, 0);
    if ((long)id <= 0) leave(1);
    expect(incoming.sender, client);
    expect(incoming.operation, operation);
    return id;
}

static word payload(word id, word offset, word destination, word length) {
    return sc(PAYLOAD, id, offset, destination, length, 0);
}

static word reply(word id, long result, word source, word length) {
    return sc(REPLY, id, result, source, length, 0);
}

/* Two pages allocated, second unmapped: test a fault on a later copy page. */
static byte *partial_mapping(void) {
    word address = sc(222, 0, 8192, 3, 0x22, (word)-1);
    if ((long)address <= 0) leave(1);
    expect(sc(215, address + 4096, 4096, 0, 0, 0), 0);
    byte *bytes = (byte *)address;
    bytes[4094] = 0x91;
    bytes[4095] = 0x92;
    return bytes;
}

__attribute__((noreturn)) void _start(void) {
    word parent = sc(172, 0, 0, 0, 0, 0);
    word child = sc(220, 17, 0, 0, 0, 0);
    if ((long)child < 0) leave(1);
    byte *source = input + 4093;
    byte *destination = output + 4093;
    byte *buffer = scratch + 4091;

    if (child == 0) {
        for (word i = 0; i < LENGTH; ++i) source[i] = (byte)i;
        expect(call(parent, WRITE, (word)source, LENGTH), LENGTH);
        expect(call(parent, READ, (word)destination, LENGTH), LENGTH);
        for (word i = 0; i < LENGTH; ++i) expect(destination[i], (byte)(i ^ 0x5a));
        expect(destination[-1], 0);
        expect(destination[LENGTH], 0);
        expect(call(parent, WRITE, 0, 4), -14);
        expect(call(parent, READ, 0, 4), -14);
        expect(call(parent, READ, (word)&_start, 4), -14);
        expect(call(parent, WRITE, (word)-3, 8), -14);
        expect(call(parent, WRITE, 0, 0), 0);
        expect(call(parent, READ, 0, 0), 0);
        expect(call(parent, CUSTOM, (word)source, 4), 4);
        for (word i = 0; i < 4; ++i) expect(source[i], i + 9);

        byte *partial = partial_mapping();
        expect(call(parent, WRITE, (word)(partial + 4094), 4), -14);
        expect(call(parent, READ, (word)(partial + 4094), 4), -14);
        /* A failed copy must not modify even the mapped portion of the range. */
        expect(partial[4094], 0x91);
        expect(partial[4095], 0x92);
        /* Scalar input/output wrappers also preflight the entire user range. */
        expect(sc(29, 0, 0x5401, (word)(partial + 4094), 0, 0), -14);
        expect(sc(134, 10, (word)(partial + 4094), 0, 8, 0), -14);
        expect(partial[4094], 0x91);
        expect(partial[4095], 0x92);

        /* Vector input/output wrappers: invalid copies must not partly write. */
        word fds[2];
        expect(sc(59, (word)fds, 0, 0, 0, 0), 0);
        expect(sc(WRITE, fds[1], (word)(partial + 4094), 4, 0, 0), -14);
        static const byte pipe_data[] = {1, 2, 3, 4};
        expect(sc(WRITE, fds[1], (word)pipe_data, 4, 0, 0), 4);
        expect(sc(READ, fds[0], (word)(partial + 4094), 4, 0, 0), -14);
        expect(partial[4094], 0x91);
        expect(partial[4095], 0x92);
        expect(sc(57, fds[0], 0, 0, 0, 0), 0);
        expect(sc(57, fds[1], 0, 0, 0, 0), 0);
        expect(sc(215, (word)partial, 4096, 0, 0, 0), 0);
        leave(0);
    }

    word id = receive(child, WRITE);
    expect(payload((word)-1, 0, (word)buffer, 1), -3);
    expect(payload(id, LENGTH, (word)-1, 0), 0);
    expect(payload(id, LENGTH, (word)buffer, 1), -22);
    expect(payload(id, (word)-1, (word)buffer, 2), -22);
    expect(payload(id, 0, 0, 1), -14);
    expect(payload(id, 0, (word)-2, 4), -14);
    expect(payload(id, 1, (word)buffer, LENGTH - 1), LENGTH - 1);
    for (word i = 0; i < LENGTH - 1; ++i) expect(buffer[i], (byte)(i + 1));
    expect(payload(id, 0, (word)buffer, LENGTH), LENGTH);
    for (word i = 0; i < LENGTH; ++i) expect(buffer[i], (byte)i);
    expect(reply(id, 1, (word)buffer, 1), -22); /* Wrong transfer direction. */
    expect(reply(id, LENGTH, 0, 0), 0);

    id = receive(child, READ);
    expect(payload(id, 0, (word)buffer, 1), -22);
    for (word i = 0; i < LENGTH; ++i) buffer[i] = (byte)(i ^ 0x5a);
    expect(reply(id, LENGTH, 0, 1), -14);
    expect(reply(id, -5, (word)buffer, 1), -22);
    expect(reply(id, LENGTH + 1, (word)buffer, LENGTH + 1), -22);
    expect(reply(id, 4, (word)-2, 4), -14);
    expect(reply(id, LENGTH, (word)buffer, LENGTH), 0);

    id = receive(child, WRITE);
    expect(payload(id, 0, (word)buffer, 4), -14);
    expect(reply(id, -14, 0, 0), 0);
    id = receive(child, READ);
    expect(reply(id, 4, (word)buffer, 4), -14);
    expect(reply(id, -14, 0, 0), 0);
    id = receive(child, READ);
    expect(reply(id, 4, (word)buffer, 4), -14);
    expect(reply(id, -14, 0, 0), 0);
    id = receive(child, WRITE);
    expect(payload(id, 4, (word)buffer, 1), -14);
    expect(reply(id, -14, 0, 0), 0);
    id = receive(child, WRITE);
    expect(payload(id, 0, (word)-1, 0), 0);
    expect(reply(id, 0, 0, 0), 0);
    id = receive(child, READ);
    expect(reply(id, 0, (word)-1, 0), 0);

    id = receive(child, CUSTOM);
    expect(payload(id, 0, (word)buffer, 4), 4);
    for (word i = 0; i < 4; ++i) expect(buffer[i], i);
    for (word i = 0; i < 4; ++i) buffer[i] = i + 9;
    expect(reply(id, 4, (word)buffer, 4), 0);
    expect(payload(id, 0, (word)buffer, 1), -3);

    id = receive(child, WRITE);
    buffer[0] = buffer[1] = 0xaa;
    expect(payload(id, 0, (word)buffer, 4), -14);
    expect(buffer[0], 0xaa);
    expect(buffer[1], 0xaa);
    expect(reply(id, -14, 0, 0), 0);
    id = receive(child, READ);
    expect(reply(id, 4, (word)buffer, 4), -14);
    expect(reply(id, -14, 0, 0), 0);

    expect(sc(95, 0, 0, (word)siginfo, 4, 0), 0);
    expect(siginfo[2], 1); /* CLD_EXITED */
    expect(siginfo[5], 0);
    static const char success[] = "IPC_PAYLOAD_OK\n";
    sc(WRITE, 1, (word)success, sizeof(success) - 1, 0, 0);
    leave(0);
}

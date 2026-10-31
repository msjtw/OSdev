/* IPC-006: concurrent clients, FIFO receives, delayed and reverse-order replies. */
typedef unsigned long word;
typedef unsigned char byte;

#define CLIENTS CASE
#define ROUNDS 3
#define CALL 0x80000001UL
#define RECEIVE 0x80000002UL
#define PAYLOAD 0x80000003UL
#define REPLY 0x80000004UL
#define OPERATION 0x80000600UL

struct Message { word sender, operation, args[6]; };
static word pids[CLIENTS];
static word requests[CLIENTS];
static word siginfo[32];
static const word delay[4] __attribute__((aligned(8))) = {0, 0, 200000000, 0};

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
    if (!condition) {
        static const char failure[] = "IPC_FIFO_FAILED\n";
        sc(64, 1, (word)failure, sizeof(failure) - 1, 0, 0);
        leave(1);
    }
}

static word result_for(word client, word round) {
    return 1000 + round * CLIENTS + client;
}

static __attribute__((noreturn)) void client(word server, word index, word ready_fd) {
    byte ready = (byte)index;
    check(sc(64, ready_fd, (word)&ready, 1, 0, 0) == 1);
    for (word round = 0; round < ROUNDS; ++round) {
        word data[2] = {index, round};
        struct Message message = {
            0, OPERATION,
            {index, (word)data, sizeof(data), round,
             0x13579bdfUL ^ index, 0x2468ace0UL ^ round},
        };
        check(sc(CALL, server, (word)&message, 0, 0, 0) == result_for(index, round));
        check(data[0] == round + 100 && data[1] == index + 100);
    }
    leave(0);
}

__attribute__((noreturn)) void _start(void) {
    word server = sc(172, 0, 0, 0, 0, 0);
    word ready_pipe[2];
    check(sc(59, (word)ready_pipe, 0, 0, 0, 0) == 0);
    for (word index = 0; index < CLIENTS; ++index) {
        word pid = sc(220, 17, 0, 0, 0, 0);
        check((long)pid >= 0);
        if (!pid) client(server, index, ready_pipe[1]);
        pids[index] = pid;
    }
    byte ready[CLIENTS];
    word count = 0;
    while (count < CLIENTS) {
        word received = sc(63, ready_pipe[0], (word)(ready + count), CLIENTS - count, 0, 0);
        check((long)received > 0);
        count += received;
    }
    /* Let ready clients enter call before receiving. Order is verified from
       kernel-assigned IDs, not guessed from PID order or scheduler timings. */
    check(sc(407, 1, 0, (word)delay, 0, 0) == 0);

    word previous_id = 0;
    for (word round = 0; round < ROUNDS; ++round) {
        word seen = 0;
        for (word received = 0; received < CLIENTS; ++received) {
            struct Message message;
            word id = sc(RECEIVE, (word)&message, 0, 0, 0, 0);
            check((long)id > 0 && id > previous_id);
            previous_id = id;
            word index = message.args[0];
            check(index < CLIENTS);
            check(!(seen & (1UL << index)));
            seen |= 1UL << index;
            requests[index] = id;
            check(message.sender == pids[index]);
            check(message.operation == OPERATION);
            check(message.args[2] == 2 * sizeof(word));
            check(message.args[3] == round);
            check(message.args[4] == (0x13579bdfUL ^ index));
            check(message.args[5] == (0x2468ace0UL ^ round));
            /* All requests remain delivered and pending until the whole batch
               has been received, including >16 entries in the larger case. */
        }
        check(seen == (1UL << CLIENTS) - 1);
        for (word reverse = CLIENTS; reverse != 0; --reverse) {
            word index = reverse - 1;
            word data[2];
            word id = requests[index];
            check(sc(PAYLOAD, id, 0, (word)data, sizeof(data), 0) == sizeof(data));
            check(data[0] == index && data[1] == round);
            data[0] = round + 100;
            data[1] = index + 100;
            check(sc(REPLY, id, result_for(index, round), (word)data, sizeof(data), 0) == 0);
            /* A duplicate completion must not disturb the client's next call. */
            check(sc(REPLY, id, 9999, 0, 0, 0) == 0);
            check(sc(PAYLOAD, id, 0, (word)data, sizeof(data), 0) == (word)-3);
        }
    }

    word reaped = 0;
    while (reaped < CLIENTS) {
        word result = sc(95, 0, 0, (word)siginfo, 4, 0);
        if (result == (word)-4) continue;
        check(result == 0);
        check(siginfo[2] == 1 && siginfo[5] == 0);
        word found = 0;
        for (word index = 0; index < CLIENTS; ++index) {
            if (pids[index] == siginfo[3]) {
                pids[index] = 0;
                found = 1;
                break;
            }
        }
        check(found);
        ++reaped;
    }
    check(sc(57, ready_pipe[0], 0, 0, 0, 0) == 0);
    check(sc(57, ready_pipe[1], 0, 0, 0, 0) == 0);
    static const char success[] = "IPC_FIFO_OK\n";
    sc(64, 1, (word)success, sizeof(success) - 1, 0, 0);
    leave(0);
}

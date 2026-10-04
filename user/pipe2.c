#include <stdio.h>
#include <unistd.h>

static int parse_fd(const char *text) {
    int value = 0;

    while (*text >= '0' && *text <= '9') {
        value = value * 10 + (*text - '0');
        text++;
    }
    return value;
}

int main(int argc, char **argv) {
    char buffer[64];
    int fd;
    int total = 0;

    if (argc != 2) {
        fprintf(stderr, "pipe2: expected a read fd\n");
        return 2;
    }

    fd = parse_fd(argv[1]);
    printf("pipe2: reading from fd %d\n", fd);

    for (;;) {
        int n = read(fd, buffer, sizeof(buffer));
        if (n < 0) {
            fprintf(stderr, "pipe2: read failed\n");
            close(fd);
            return 1;
        }
        if (n == 0) {
            break;
        }

        if (write(1, buffer, n) != n) {
            fprintf(stderr, "pipe2: stdout write failed\n");
            close(fd);
            return 1;
        }
        total += n;
    }

    close(fd);
    printf("pipe2: EOF after %d bytes\n", total);
    return 0;
}

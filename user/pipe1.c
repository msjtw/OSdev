#include <stdio.h>
#include <sys/wait.h>
#include <unistd.h>

static int write_all(int fd, const char *data, int len) {
    int written = 0;

    while (written < len) {
        int n = write(fd, data + written, len - written);
        if (n <= 0) {
            return -1;
        }
        written += n;
    }

    return 0;
}

int main(void) {
    static const char message[] = "pipe test: hello from pipe1\n";
    int pipefd[2];

    if (pipe(pipefd) < 0) {
        fprintf(stderr, "pipe1: pipe failed\n");
        return 1;
    }

    int pid = fork();
    if (pid < 0) {
        fprintf(stderr, "pipe1: fork failed\n");
        close(pipefd[0]);
        close(pipefd[1]);
        return 1;
    }

    if (pid == 0) {
        char read_fd[12];
        sprintf(read_fd, "%d", pipefd[0]);

        close(pipefd[1]);
        execlp("pipe2", "pipe2", read_fd, NULL);
        fprintf(stderr, "pipe1: exec pipe2 failed\n");
        _exit(127);
    }

    close(pipefd[0]);
    if (write_all(pipefd[1], message, sizeof(message) - 1) < 0) {
        fprintf(stderr, "pipe1: write failed\n");
    }
    close(pipefd[1]); // pipe2 should observe EOF after reading the message.

    wait(0);
    printf("pipe1: test complete\n");
    return 0;
}

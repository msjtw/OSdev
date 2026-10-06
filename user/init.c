#include <stdio.h>
#include <termios.h>
#include <unistd.h>
#include <sys/wait.h>

int main(void) {
    struct termios termios;

    if (tcgetattr(STDIN_FILENO, &termios) < 0) {
        fprintf(stderr, "init: tcgetattr failed\n");
        return 1;
    }
    termios.c_lflag |= ECHO;
    if (tcsetattr(STDIN_FILENO, TCSANOW, &termios) < 0) {
        fprintf(stderr, "init: tcsetattr failed\n");
        return 1;
    }

    for (int i = 1;; i++) {
        char input[32];
        int pid;

        printf("pipe cleanup test %d\n", i);

        pid = fork();
        if (pid == 0) {
            execlp("pipe1", "pipe1", NULL);
            fprintf(stderr, "init: exec pipe1 failed\n");
            _exit(127);
        }

        if (pid < 0) {
            fprintf(stderr, "init: fork failed\n");
            return 1;
        }

        wait(NULL);

        printf("init: type a word for stdin test: ");
        fflush(stdout);
        if (scanf("%31s", input) != 1) {
            fprintf(stderr, "init: stdin read failed\n");
            return 1;
        }
        printf("init: read \"%s\" from stdin\n", input);
    }
}

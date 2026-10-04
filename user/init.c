#include <stdio.h>
#include <unistd.h>
#include <sys/wait.h>

int main(void) {
    for (int i = 1;; i++) {
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
    }
}

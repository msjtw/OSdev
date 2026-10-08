#include <stdio.h>
#include <sys/wait.h>
#include <unistd.h>

int main(void) {
    for (int i = 1;; i++) {
        int pid = fork();
        if (pid < 0) {
            fprintf(stderr, "fork_prime: fork failed\n");
            return 1;
        }

        if (pid == 0) {
            char nth[12];

            snprintf(nth, sizeof(nth), "%d", i);
            execlp("prime", nth, NULL);
            fprintf(stderr, "fork_prime: exec prime failed\n");
            _exit(127);
        }

        if (wait(NULL) < 0) {
            fprintf(stderr, "fork_prime: wait failed\n");
            return 1;
        }
    }
}

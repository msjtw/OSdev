#include <stdio.h>
#include <string.h>
#include <sys/types.h>
#include <unistd.h>
#include <stdio.h>
#include <unistd.h>
#include <sys/types.h>
#include <sys/wait.h>

int main() {
    for (int i = 1;; i++) {
        printf("calculating %d-th prime: \n", i);
        if(!fork()){
            // child
            int n;
            scanf("%d", &n);
            char buff[20];
            memset(buff, 0, 20);
            sprintf(buff, "%d", n);
            execlp("prime", buff, NULL);
        } else{
            //parent
            wait(0);
        }
    }
}

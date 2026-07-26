#include "user.h"

void sleep(int n) {
    while (n-- > 0) {
        int k = 1000000000;
        while (k-- > 0) {
            k--;
        }
    }
}

int rem(int a, int b) {
    while (a - b >= 0) {
        a -= b;
    }
    return a;
}

int main(int argc, char *argv[]) {
    int divi = atoi(argv[0]);
    // int maxi = atoi(argv[1]);
    for (int i = 0;; i++) {
        int r = rem(i, divi);
        printf("%d %% %d = %d\n", i, divi, r);
        sleep(1000000000);
    }
}

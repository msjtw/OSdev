#include <stdio.h>
#include <stdlib.h>

int is_prime(int a) {
    if (a <= 1)
        return 0;
    if (a == 2)
        return 1;

    for (int i = 2; i * i <= a; i++) {
        if (a % i == 0) {
            return 0;
        }
    }

    return 1;
}

int main(int argc, char *argv[]) {
    int n = atoi(argv[0]);
    int prime = 0;
    int i = n;
    while (i > 0) {
        prime++;
        if (is_prime(prime)) {
            i--;
        }
    }
    printf("%d-th prime is: %d\n", n, prime);

    return 0;
}

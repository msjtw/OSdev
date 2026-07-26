#include "user.h"

void sleep (int n){
    while (n --> 0){
        int k = 1000000000;
        while (k --> 0){
            k--;
        }
    }
}

int rem(int a, int b) {
    while (a-b >= 0) {
        a -= b;
    }
    return a;
}

int main(int argc, char *argv[])
{
    for(int i = 0;; i++){
        int r = rem(i, 7);
        sleep(10000000*r);
    }
}


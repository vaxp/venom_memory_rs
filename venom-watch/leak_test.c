#include <stdlib.h>
#include <stdio.h>

void normal_flow() {
    int *p = malloc(sizeof(int));
    if (p) {
        *p = 10;
        free(p); // ✅ Correct
    }
}

void simple_leak() {
    int *data = malloc(1024); // ❌ Leaked! No free here.
    printf("Doing something with data...\n");
}

void double_allocation_leak() {
    char *ptr = malloc(10); 
    ptr = malloc(20); // ❌ First pointer is leaked!
    free(ptr);
}

void conditional_leak(int condition) {
    char *buf = malloc(100);
    if (condition) {
        free(buf);
    }
    // ❌ Leaked if condition is false!
}

int main() {
    normal_flow();
    simple_leak();
    double_allocation_leak();
    conditional_leak(0);
    return 0;
}

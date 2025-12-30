#include <stdlib.h>
#include <stdio.h>

void test_uaf() {
    int *ptr = malloc(sizeof(int));
    *ptr = 42;
    free(ptr);
    
    printf("Value after free: %d\n", *ptr); // 💀 Use-After-Free
}

void test_double_free() {
    void *buf = malloc(100);
    free(buf);
    free(buf); // 🚫 Double Free
}

void test_use_after_move() {
    char *str = malloc(64);
    // @Venom:Owns(str)
    some_external_cleanup(str);
    
    printf("Str is: %s\n", str); // 💀 Use-After-Move (Treated as UAF)
}

int main() {
    test_uaf();
    test_double_free();
    test_use_after_move();
    return 0;
}

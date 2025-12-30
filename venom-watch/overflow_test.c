#include <stdio.h>

void test_overflow() {
    int arr[5];
    arr[0] = 1;
    arr[4] = 5;
    arr[5] = 10; // ⚠️  Buffer Overflow
}

void test_another_overflow() {
    char buf[10];
    buf[0] = 'a';
    buf[10] = 'b'; // ⚠️  Buffer Overflow
}

int main() {
    test_overflow();
    test_another_overflow();
    return 0;
}

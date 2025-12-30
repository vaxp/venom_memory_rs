#include <stdio.h>

void test_loop_oboe() {
    int arr[5];
    // Off-By-One: i will reach 5, but max index is 4.
    for (int i = 0; i <= 5; i++) {
        arr[i] = i; // ⚠️  SHOULD FLAG
    }
}

void test_safe_loop() {
    int buf[10];
    for (int j = 0; j < 10; j++) {
        buf[j] = j; // Should NOT flag
    }
}

int main() {
    test_loop_oboe();
    test_safe_loop();
    return 0;
}

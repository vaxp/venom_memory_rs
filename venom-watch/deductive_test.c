#include <stdio.h>

void test_deductive(int idx) {
    int arr[5];
    
    if (idx < 5) {
        printf("Safe access: %d\n", arr[idx]); // Should NOT flag
    } else {
        printf("Dangerous access: %d\n", arr[idx]); // ⚠️  SHOULD FLAG (idx >= 5)
    }
}

void test_edge_case(int idx) {
    int buffer[10];
    if (idx >= 10) {
        buffer[idx] = 0; // ⚠️  SHOULD FLAG
    } else {
        buffer[idx] = 0; // Should NOT flag
    }
}

int main() {
    test_deductive(10);
    test_edge_case(10);
    return 0;
}

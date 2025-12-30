#include <stdlib.h>
#include <stdio.h>

void print_data(void *ptr) {
    if (ptr) printf("Data info...\n");
}

void destroy_data(void *ptr) {
    if (ptr) free(ptr);
}

void custom_handler(void *ptr) {
    // Some complex logic where ownership is taken
    printf("Handled elsewhere.\n");
}

void test_borrowing() {
    int *p1 = malloc(10);
    print_data(p1); 
    // ❌ Should report high-confidence leak (print_data is a borrow)
}

void test_heuristic() {
    int *p2 = malloc(20);
    destroy_data(p2);
    // ⚠️ Should report 50% warning (destroy_data has 'destroy' keyword)
}

void test_annotation() {
    int *p3 = malloc(30);
    // @Venom:Owns(p3)
    custom_handler(p3);
    // ✅ Should be silent (0% warning) because of annotation
}

int main() {
    test_borrowing();
    test_heuristic();
    test_annotation();
    return 0;
}

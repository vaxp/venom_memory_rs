#include <stdlib.h>
#include <stdio.h>

void cleanup_data(void *ptr) {
    if (ptr) {
        printf("Cleaning up memory at %p\n", ptr);
        free(ptr);
    }
}

void process_with_transfer() {
    int *secret_data = malloc(1024);
    if (!secret_data) return;

    // ... process data ...
    
    // Ownership is transferred to cleanup_data
    cleanup_data(secret_data); 
    
    // No explicit free() here, but it's handled!
}

int main() {
    process_with_transfer();
    return 0;
}

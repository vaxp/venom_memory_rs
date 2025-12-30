#include <stdint.h>

struct PaddingStruct {
    char a;      // 1 byte, offset 0
    // 3 bytes padding (int requires 4-byte alignment)
    int b;       // 4 bytes, offset 4
    char c;      // 1 byte, offset 8
    // 7 bytes padding (double requires 8-byte alignment)
    double d;    // 8 bytes, offset 16
    char e;      // 1 byte, offset 24
    // 7 bytes trailing padding (struct size must be multiple of 8)
}; // Total: 32 bytes

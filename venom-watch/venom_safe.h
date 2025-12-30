#ifndef VENOM_SAFE_H
#define VENOM_SAFE_H

#include <stdlib.h>
#include <stdio.h>

/**
 * --- VENOM SMART POINTERS (RAII) ---
 * Automatically frees the pointer when it goes out of scope.
 */

static inline void venom_internal_free(void *ptr) {
    void **p = (void **)ptr;
    if (p && *p) {
        // printf("[VenomSafe] Auto-freeing memory at %p\n", *p);
        free(*p);
        *p = NULL;
    }
}

// Declares a pointer that will be automatically freed.
// Usage: vptr int *p = malloc(sizeof(int));
#define vptr __attribute__((cleanup(venom_internal_free)))

// Standard header for every Venom Object
#define VENOM_OBJECT \
    void (*destroy)(void*);

static inline void venom_internal_obj_delete(void *ptr) {
    void **p = (void **)ptr;
    if (p && *p) {
        // Every Venom object MUST have VENOM_OBJECT at the beginning.
        struct { VENOM_OBJECT } *obj = *p;
        if (obj->destroy) {
            obj->destroy(*p);
        }
        free(*p);
        *p = NULL;
    }
}

// Declares an object pointer that will be automatically destroyed and freed.
#define vobj __attribute__((cleanup(venom_internal_obj_delete)))

/**
 * --- VENOM OOP INFRASTRUCTURE ---
 */

// Define a Class Structure with VENOM_OBJECT at the start
#define CLASS(name) \
    typedef struct name name; \
    struct name { \
        VENOM_OBJECT

// Define a Method within a Class (Function Pointer)
#define METHOD(ret, name, ...) \
    ret (*name)(void *self, ##__VA_ARGS__)

// Constructor call helper
#define NEW(type, ...) \
    type##_new(__VA_ARGS__)

// Destructor call helper
#define DELETE(obj) \
    if (obj) { \
        obj->destroy(obj); \
        free(obj); \
    }

#endif // VENOM_SAFE_H

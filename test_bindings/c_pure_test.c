#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <pthread.h>
#include <unistd.h>
#include <stdatomic.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <fcntl.h>
#include <stdint.h>

#define DATA_SIZE (256 * 1024)
#define NUM_CHANNELS 4
#define ITERATIONS 500000

// Match ultra_test.rs ChannelData exactly
typedef struct __attribute__((aligned(64))) {
    _Atomic uint64_t write_seq;
    _Atomic uint64_t read_seq;
    _Atomic uint64_t data_len;
    char _pad[40];
} ChannelData;

typedef struct {
    int id;
    void* ptr;
    size_t size;
} ThreadArgs;

typedef struct {
    uint64_t successful;
    double total_latency_ns;
    double min_latency_ns;
    double max_latency_ns;
} Stats;

Stats stats[NUM_CHANNELS];
pthread_barrier_t start_barrier;
_Atomic int stop_flag = 0;

static inline double get_time_ns() {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (double)ts.tv_sec * 1e9 + (double)ts.tv_nsec;
}

void* create_shm(const char* name, size_t size) {
    char full_name[128];
    snprintf(full_name, sizeof(full_name), "/venom_%s", name);
    
    shm_unlink(full_name); // Clean up old
    
    int fd = shm_open(full_name, O_CREAT | O_RDWR, 0666);
    if (fd < 0) { perror("shm_open"); return NULL; }
    
    if (ftruncate(fd, size) < 0) { perror("ftruncate"); return NULL; }
    
    void* ptr = mmap(NULL, size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    close(fd);
    
    if (ptr == MAP_FAILED) { perror("mmap"); return NULL; }
    
    memset(ptr, 0, size);
    return ptr;
}

void* daemon_thread(void* arg) {
    ThreadArgs* args = (ThreadArgs*)arg;
    ChannelData* header = (ChannelData*)args->ptr;
    uint8_t* data_ptr = (uint8_t*)args->ptr + sizeof(ChannelData);
    
    uint64_t last_read_seq = 0;
    
    pthread_barrier_wait(&start_barrier);
    
    while (!atomic_load(&stop_flag)) {
        uint64_t read_seq = atomic_load_explicit(&header->read_seq, memory_order_acquire);
        if (read_seq > last_read_seq) {
            last_read_seq = read_seq;
            
            // Write response
            atomic_fetch_add_explicit(&header->write_seq, 1, memory_order_release); // Odd
            
            // Actual data copy (like Rust copy_nonoverlapping)
            memset(data_ptr, (uint8_t)read_seq, DATA_SIZE);
            atomic_store_explicit(&header->data_len, DATA_SIZE, memory_order_relaxed);
            
            atomic_fetch_add_explicit(&header->write_seq, 1, memory_order_release); // Even
        } else {
            __asm__ __volatile__ ("pause" ::: "memory");
        }
    }
    return NULL;
}

void* shell_thread(void* arg) {
    ThreadArgs* args = (ThreadArgs*)arg;
    ChannelData* header = (ChannelData*)args->ptr;
    uint8_t* data_ptr = (uint8_t*)args->ptr + sizeof(ChannelData);
    uint8_t* read_buf = malloc(DATA_SIZE);
    
    Stats* s = &stats[args->id];
    s->min_latency_ns = 1e18;
    s->max_latency_ns = 0;
    s->total_latency_ns = 0;
    s->successful = 0;
    
    pthread_barrier_wait(&start_barrier);
    
    for (uint64_t i = 0; i < ITERATIONS; i++) {
        double start = get_time_ns();
        
        // Send request
        atomic_fetch_add_explicit(&header->read_seq, 1, memory_order_release);
        
        // Wait for response
        while (1) {
            uint64_t seq1 = atomic_load_explicit(&header->write_seq, memory_order_acquire);
            if (seq1 & 1) { __asm__ __volatile__ ("pause" ::: "memory"); continue; }
            
            // Read data
            memcpy(read_buf, data_ptr, DATA_SIZE);
            
            atomic_thread_fence(memory_order_acquire);
            
            uint64_t seq2 = atomic_load_explicit(&header->write_seq, memory_order_acquire);
            if (seq1 == seq2 && seq1 > 0) break;
        }
        
        double end = get_time_ns();
        double lat = end - start;
        
        s->total_latency_ns += lat;
        if (lat < s->min_latency_ns) s->min_latency_ns = lat;
        if (lat > s->max_latency_ns) s->max_latency_ns = lat;
        s->successful++;
    }
    
    free(read_buf);
    return NULL;
}

int main() {
    printf("╔═══════════════════════════════════════════════════════════════╗\n");
    printf("║   VenomMemory C - Pure POSIX Test (Match Rust ultra_test)     ║\n");
    printf("╚═══════════════════════════════════════════════════════════════╝\n\n");

    pthread_barrier_init(&start_barrier, NULL, NUM_CHANNELS * 2);

    pthread_t daemons[NUM_CHANNELS], shells[NUM_CHANNELS];
    ThreadArgs d_args[NUM_CHANNELS], s_args[NUM_CHANNELS];
    
    size_t region_size = sizeof(ChannelData) + DATA_SIZE;
    
    printf("Creating %d channels...\n", NUM_CHANNELS);
    for (int i = 0; i < NUM_CHANNELS; i++) {
        char name[32];
        sprintf(name, "pure_ch_%d", i);
        
        void* ptr = create_shm(name, region_size);
        if (!ptr) { fprintf(stderr, "Failed to create shm %d\n", i); return 1; }
        
        d_args[i].id = i;
        d_args[i].ptr = ptr;
        d_args[i].size = region_size;
        
        s_args[i].id = i;
        s_args[i].ptr = ptr;
        s_args[i].size = region_size;
    }
    
    printf("Starting threads...\n");
    double start_time = get_time_ns();
    
    for (int i = 0; i < NUM_CHANNELS; i++) {
        pthread_create(&daemons[i], NULL, daemon_thread, &d_args[i]);
        pthread_create(&shells[i], NULL, shell_thread, &s_args[i]);
    }
    
    for (int i = 0; i < NUM_CHANNELS; i++) {
        pthread_join(shells[i], NULL);
    }
    
    atomic_store(&stop_flag, 1);
    for (int i = 0; i < NUM_CHANNELS; i++) {
        pthread_join(daemons[i], NULL);
    }
    
    double end_time = get_time_ns();
    double duration_sec = (end_time - start_time) / 1e9;

    // Results
    printf("\n┌─────────┬───────────┬──────────┬──────────────┐\n");
    printf("│ Channel │ Successful│ Avg (µs) │ Max (µs)     │\n");
    printf("├─────────┼───────────┼──────────┼──────────────┤\n");
    
    uint64_t total = 0;
    double total_lat = 0, global_min = 1e18, global_max = 0;
    
    for (int i = 0; i < NUM_CHANNELS; i++) {
        Stats* s = &stats[i];
        total += s->successful;
        total_lat += s->total_latency_ns;
        if (s->min_latency_ns < global_min) global_min = s->min_latency_ns;
        if (s->max_latency_ns > global_max) global_max = s->max_latency_ns;
        
        printf("│    %d    │  %8lu  │  %7.2f │  %11.2f │\n",
            i, s->successful, 
            s->total_latency_ns / s->successful / 500000.0,
            s->max_latency_ns / 500000.0);
    }
    printf("└─────────┴───────────┴──────────┴──────────────┘\n");
    
    double throughput = total / duration_sec;
    double bandwidth_mb = throughput * DATA_SIZE * 2.0 / 1e6;
    
    printf("\n📊 AGGREGATE RESULTS:\n");
    printf("   Channels:         %d\n", NUM_CHANNELS);
    printf("   Total successful: %lu / %lu\n", total, (uint64_t)NUM_CHANNELS * ITERATIONS);
    printf("   Test duration:    %.2f seconds\n", duration_sec);
    printf("   Avg latency:      %.2f µs\n", total_lat / total / 500000.0);
    printf("   Min latency:      %.2f µs\n", global_min / 500000.0);
    printf("   Max latency:      %.2f µs (%.2f ms)\n", global_max / 500000.0, global_max / 1e6);
    printf("   ⚡ THROUGHPUT:     %.0f req/s\n", throughput);
    printf("   📶 BANDWIDTH:      %.2f MB/s = %.2f GB/s\n", bandwidth_mb, bandwidth_mb / 500000.0);

    // Cleanup
    for (int i = 0; i < NUM_CHANNELS; i++) {
        char name[64];
        sprintf(name, "/venom_pure_ch_%d", i);
        shm_unlink(name);
    }

    return 0;
}

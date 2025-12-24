#include "venom_memory_rs.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <pthread.h>
#include <unistd.h>

#define DATA_SIZE (256 * 1024)
#define ITERATIONS 100000
#define NUM_CHANNELS 4

typedef struct {
    int id;
    VenomDaemonHandle* daemon;
} DaemonThreadArgs;

typedef struct {
    int id;
    VenomShellHandle* shell;
    double* throughput;
} ShellThreadArgs;

void* daemon_thread(void* arg) {
    DaemonThreadArgs* args = (DaemonThreadArgs*)arg;
    uint8_t* data = malloc(DATA_SIZE);
    
    // Use first 8 bytes as sequence number
    uint64_t* seq_ptr = (uint64_t*)data;

    for (int i = 0; i < ITERATIONS; i++) {
        *seq_ptr = (uint64_t)(i + 1); // 1-based sequence
        // Fill rest with some data if needed, but memset is slow in loop
        // Just rely on seq for verification
        venom_daemon_write_data(args->daemon, data, DATA_SIZE);
        
        // Small delay to allow readers to catch up? 
        // No, we want to test max throughput. 
        // In SWMR, readers might miss frames if writer is too fast.
        // But for benchmark we want to see how many frames we ACTUALLY caught.
    }
    
    // Signal end
    *seq_ptr = 0xFFFFFFFFFFFFFFFF;
    venom_daemon_write_data(args->daemon, data, DATA_SIZE);
    
    free(data);
    return NULL;
}

void* shell_thread(void* arg) {
    ShellThreadArgs* args = (ShellThreadArgs*)arg;
    uint8_t* val_buf = malloc(DATA_SIZE + 64);
    uint64_t last_seq = 0;
    uint64_t received_count = 0;
    
    struct timespec start, end;
    clock_gettime(CLOCK_MONOTONIC, &start);

    // We run for a fixed time or until daemon signals end
    // But since daemon is free-running, we might miss frames.
    // Let's count how many UNIQUE frames we received.
    
    while (1) {
        size_t len = venom_shell_read_data(args->shell, val_buf, DATA_SIZE + 64);
        if (len < 8) continue;
        
        uint64_t current_seq = *(uint64_t*)val_buf;
        
        if (current_seq == 0xFFFFFFFFFFFFFFFF) break;
        
        if (current_seq > last_seq) {
            received_count++;
            last_seq = current_seq;
        } else {
             // Busy wait / spin
             // We are reading faster than writing, or seeing old data
        }
    }

    clock_gettime(CLOCK_MONOTONIC, &end);
    
    double elapsed = (end.tv_sec - start.tv_sec) + (end.tv_nsec - start.tv_nsec) / 1e9;
    // Throughput is based on Received Unique Frames
    *args->throughput = (double)received_count / elapsed;
    
    printf("Shell %d received %lu / %d frames (Latency/Loss test)\n", 
           args->id, received_count, ITERATIONS);
    
    free(val_buf);
    return NULL;
}

int main() {
    printf("Initializing C Benchmark for VenomMemory Bindings...\n");

    pthread_t daemons[NUM_CHANNELS];
    pthread_t shells[NUM_CHANNELS];
    
    DaemonThreadArgs d_args[NUM_CHANNELS];
    ShellThreadArgs s_args[NUM_CHANNELS];
    double throughputs[NUM_CHANNELS];

    VenomConfig config = {
        .data_size = DATA_SIZE,
        .cmd_slots = 64,
        .max_clients = 16
    };

    // Create Channels
    for (int i = 0; i < NUM_CHANNELS; i++) {
        char name[64];
        sprintf(name, "c_bench_%d", i);
        
        d_args[i].id = i;
        d_args[i].daemon = venom_daemon_create(name, config);
        
        if (!d_args[i].daemon) {
            fprintf(stderr, "Failed to create daemon %d\n", i);
            return 1;
        }

        s_args[i].id = i;
        s_args[i].shell = venom_shell_connect(name);
        s_args[i].throughput = &throughputs[i];
        
        if (!s_args[i].shell) {
            fprintf(stderr, "Failed to connect shell %d\n", i);
            return 1;
        }
    }

    printf("Starting threads (4 channels, 256KB data, 100k iters)...\n");

    // Start Threads
    for (int i = 0; i < NUM_CHANNELS; i++) {
        pthread_create(&daemons[i], NULL, daemon_thread, &d_args[i]);
        pthread_create(&shells[i], NULL, shell_thread, &s_args[i]);
    }

    // Join
    for (int i = 0; i < NUM_CHANNELS; i++) {
        pthread_join(daemons[i], NULL);
        pthread_join(shells[i], NULL);
    }

    // Report
    double total_throughput = 0;
    for (int i = 0; i < NUM_CHANNELS; i++) {
        total_throughput += throughputs[i];
    }
    
    double bandwidth_mb = total_throughput * DATA_SIZE / 1e6;
    double bandwidth_gb = bandwidth_mb / 1000.0;

    printf("\nResults:\n");
    printf("Total Throughput: %.0f req/s\n", total_throughput);
    printf("Total Bandwidth:  %.2f GB/s\n", bandwidth_gb);
    
    // Cleanup
    for (int i = 0; i < NUM_CHANNELS; i++) {
        venom_shell_destroy(s_args[i].shell);
        venom_daemon_destroy(d_args[i].daemon);
    }

    return 0;
}

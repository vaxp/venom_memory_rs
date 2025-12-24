#ifndef VENOM_MEMORY_RS_H
#define VENOM_MEMORY_RS_H

#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>

typedef struct VenomDaemonHandle VenomDaemonHandle;
typedef struct VenomShellHandle VenomShellHandle;

typedef struct {
    size_t data_size;
    size_t cmd_slots;
    size_t max_clients;
} VenomConfig;

#ifdef __cplusplus
extern "C" {
#endif

// Daemon
VenomDaemonHandle* venom_daemon_create(const char* name, VenomConfig config);
void venom_daemon_destroy(VenomDaemonHandle* handle);
void venom_daemon_write_data(VenomDaemonHandle* handle, const uint8_t* data, size_t len);
uint8_t* venom_daemon_get_shm_ptr(VenomDaemonHandle* handle);

// Shell
VenomShellHandle* venom_shell_connect(const char* name);
void venom_shell_destroy(VenomShellHandle* handle);
size_t venom_shell_read_data(VenomShellHandle* handle, uint8_t* buf, size_t max_len);
uint32_t venom_shell_id(VenomShellHandle* handle);
bool venom_shell_send_command(VenomShellHandle* handle, const uint8_t* cmd, size_t len);
const uint8_t* venom_shell_get_shm_ptr(VenomShellHandle* handle);

#ifdef __cplusplus
}
#endif

#endif // VENOM_MEMORY_RS_H

#ifndef CODEX_APP_SERVER_IOS_H
#define CODEX_APP_SERVER_IOS_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

#define CODEX_IOS_PLATFORM_CALLBACKS_VERSION 1

typedef size_t (*codex_ios_unsupported_operation_callback_t)(
    void *context,
    const char *operation,
    const char *payload_json,
    char *message_buffer,
    size_t message_buffer_len);

typedef struct CodexIosPlatformCallbacks {
    uint32_t version;
    void *context;
    codex_ios_unsupported_operation_callback_t unsupported_operation;
} CodexIosPlatformCallbacks;

// Registers callbacks for iOS platform operations that are normally backed by
// APIs unavailable to an embedded iOS process. Passing NULL clears callbacks.
// Unsupported-operation callbacks may write a UTF-8 message to message_buffer
// and return the number of bytes written, excluding the trailing nul. Returning
// 0 means the operation was not handled.
//
// The callback table is copied. The context pointer remains owned by Swift and
// must stay valid until callbacks are cleared or replaced.
void codex_ios_platform_set_callbacks(
    const CodexIosPlatformCallbacks *callbacks);

// Clears registered iOS platform callbacks.
void codex_ios_platform_clear_callbacks(void);

// Starts codex-app-server in stdio transport mode.
//
// The caller transfers ownership of both file descriptors to Rust. When using
// Foundation Pipe, pass the read file descriptor from the app-to-Codex pipe and
// the write file descriptor from the Codex-to-app pipe. Duplicate file
// descriptors first if Swift should continue owning the original FileHandle.
//
// Returns a nonzero handle on success, or 0 if the runtime thread could not be
// started.
uint64_t codex_app_server_ios_start_with_stdio_fds(
    int input_read_fd,
    int output_write_fd);

// Requests shutdown without waiting for the runtime thread to exit.
//
// Returns 0 if the request was delivered, or -1 for an unknown handle.
int32_t codex_app_server_ios_request_stop(uint64_t handle_id);

// Requests shutdown and waits for the runtime thread to exit.
//
// Returns 0 on clean shutdown, -1 for an unknown handle, -2 if app-server
// returned an error, or -3 if the runtime thread panicked.
int32_t codex_app_server_ios_stop(uint64_t handle_id);

#ifdef __cplusplus
}
#endif

#endif

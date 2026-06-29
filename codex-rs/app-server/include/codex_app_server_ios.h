#ifndef CODEX_APP_SERVER_IOS_H
#define CODEX_APP_SERVER_IOS_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define CODEX_IOS_PLATFORM_CALLBACKS_VERSION 3

typedef uint64_t CodexIosRequestId;
typedef uint64_t CodexIosHandle;

typedef struct CodexIosString {
    const char *ptr;
    size_t len;
} CodexIosString;

typedef struct CodexIosStringArray {
    const CodexIosString *values;
    size_t len;
} CodexIosStringArray;

typedef struct CodexIosEnvVar {
    CodexIosString key;
    CodexIosString value;
} CodexIosEnvVar;

typedef struct CodexIosEnvVarArray {
    const CodexIosEnvVar *values;
    size_t len;
} CodexIosEnvVarArray;

typedef struct CodexIosErrorBuffer {
    char *message_buffer;
    size_t message_buffer_len;
} CodexIosErrorBuffer;

typedef enum CodexIosStatus {
    CodexIosStatusAccepted = 0,
    CodexIosStatusUnsupported = 1,
    CodexIosStatusError = 2,
} CodexIosStatus;

typedef enum CodexIosProcessPurpose {
    CodexIosProcessPurposeCommandExec = 0,
    CodexIosProcessPurposeProcessSpawn = 1,
    CodexIosProcessPurposeShellTool = 2,
    CodexIosProcessPurposeFsSandbox = 3,
    CodexIosProcessPurposeHookCommand = 4,
    CodexIosProcessPurposeMcpStdio = 5,
} CodexIosProcessPurpose;

typedef struct CodexIosTerminalSize {
    uint16_t cols;
    uint16_t rows;
} CodexIosTerminalSize;

typedef struct CodexIosProcessSpawnRequest {
    CodexIosRequestId request_id;
    CodexIosProcessPurpose purpose;
    CodexIosString program;
    CodexIosStringArray args;
    CodexIosString cwd;
    CodexIosEnvVarArray env;
    CodexIosString arg0;
    bool tty;
    bool stream_stdin;
    CodexIosTerminalSize size;
} CodexIosProcessSpawnRequest;

typedef struct CodexIosGitRunRequest {
    CodexIosString cwd;
    CodexIosString program;
    CodexIosStringArray args;
    CodexIosEnvVarArray env;
} CodexIosGitRunRequest;

typedef struct CodexIosGitApplyRequest {
    CodexIosString cwd;
    CodexIosStringArray git_config;
    CodexIosStringArray args;
    CodexIosString diff;
} CodexIosGitApplyRequest;

typedef struct CodexIosGitStageRequest {
    CodexIosString git_root;
    CodexIosString diff;
} CodexIosGitStageRequest;

typedef struct CodexIosShellSnapshotRequest {
    CodexIosString shell;
    CodexIosString script;
    CodexIosString cwd;
    bool login_shell;
    uint64_t timeout_ms;
} CodexIosShellSnapshotRequest;

typedef struct CodexIosMcpStdioRequest {
    CodexIosString program;
    CodexIosStringArray args;
    CodexIosString cwd;
} CodexIosMcpStdioRequest;

typedef struct CodexIosDoctorReportRequest {
    CodexIosString codex_home;
} CodexIosDoctorReportRequest;

typedef size_t (*codex_ios_unsupported_operation_callback_t)(
    void *context,
    const char *operation,
    const char *payload_json,
    char *message_buffer,
    size_t message_buffer_len);

typedef void (*codex_ios_log_message_callback_t)(
    void *context,
    const char *level,
    const char *target,
    const char *message);

typedef CodexIosStatus (*codex_ios_process_spawn_callback_t)(
    void *context,
    const CodexIosProcessSpawnRequest *request,
    CodexIosErrorBuffer error);

typedef CodexIosStatus (*codex_ios_process_write_stdin_callback_t)(
    void *context,
    CodexIosHandle handle,
    const uint8_t *bytes,
    size_t len,
    bool close_stdin,
    CodexIosErrorBuffer error);

typedef CodexIosStatus (*codex_ios_process_resize_pty_callback_t)(
    void *context,
    CodexIosHandle handle,
    CodexIosTerminalSize size,
    CodexIosErrorBuffer error);

typedef CodexIosStatus (*codex_ios_process_terminate_callback_t)(
    void *context,
    CodexIosHandle handle,
    CodexIosErrorBuffer error);

typedef CodexIosStatus (*codex_ios_code_mode_create_session_callback_t)(
    void *context,
    CodexIosRequestId request_id,
    CodexIosErrorBuffer response_json);

typedef CodexIosStatus (*codex_ios_code_mode_execute_callback_t)(
    void *context,
    CodexIosHandle session_handle,
    CodexIosString request_json,
    CodexIosErrorBuffer response_json);

typedef CodexIosStatus (*codex_ios_code_mode_wait_callback_t)(
    void *context,
    CodexIosHandle session_handle,
    CodexIosString cell_id,
    uint64_t yield_time_ms,
    CodexIosErrorBuffer response_json);

typedef CodexIosStatus (*codex_ios_code_mode_terminate_callback_t)(
    void *context,
    CodexIosHandle session_handle,
    CodexIosString cell_id,
    CodexIosErrorBuffer response_json);

typedef CodexIosStatus (*codex_ios_code_mode_shutdown_callback_t)(
    void *context,
    CodexIosHandle session_handle,
    CodexIosErrorBuffer error);

typedef CodexIosStatus (*codex_ios_git_run_callback_t)(
    void *context,
    const CodexIosGitRunRequest *request,
    CodexIosErrorBuffer response_json);

typedef CodexIosStatus (*codex_ios_git_apply_callback_t)(
    void *context,
    const CodexIosGitApplyRequest *request,
    CodexIosErrorBuffer response_json);

typedef CodexIosStatus (*codex_ios_git_stage_callback_t)(
    void *context,
    const CodexIosGitStageRequest *request,
    CodexIosErrorBuffer error);

typedef CodexIosStatus (*codex_ios_shell_snapshot_callback_t)(
    void *context,
    const CodexIosShellSnapshotRequest *request,
    CodexIosErrorBuffer response);

typedef CodexIosStatus (*codex_ios_mcp_stdio_launch_callback_t)(
    void *context,
    const CodexIosMcpStdioRequest *request,
    CodexIosErrorBuffer error);

typedef CodexIosStatus (*codex_ios_doctor_report_callback_t)(
    void *context,
    const CodexIosDoctorReportRequest *request,
    CodexIosErrorBuffer response_json);

typedef struct CodexIosPlatformCallbacks {
    uint32_t version;
    size_t struct_size;
    void *context;
    codex_ios_unsupported_operation_callback_t unsupported_operation;
    codex_ios_log_message_callback_t log_message;
    codex_ios_process_spawn_callback_t process_spawn;
    codex_ios_process_write_stdin_callback_t process_write_stdin;
    codex_ios_process_resize_pty_callback_t process_resize_pty;
    codex_ios_process_terminate_callback_t process_terminate;
    codex_ios_code_mode_create_session_callback_t code_mode_create_session;
    codex_ios_code_mode_execute_callback_t code_mode_execute;
    codex_ios_code_mode_wait_callback_t code_mode_wait;
    codex_ios_code_mode_terminate_callback_t code_mode_terminate;
    codex_ios_code_mode_shutdown_callback_t code_mode_shutdown;
    codex_ios_git_run_callback_t git_run;
    codex_ios_git_apply_callback_t git_apply_patch;
    codex_ios_git_stage_callback_t git_stage_paths;
    codex_ios_shell_snapshot_callback_t shell_snapshot;
    codex_ios_mcp_stdio_launch_callback_t mcp_stdio_launch;
    codex_ios_doctor_report_callback_t doctor_report;
} CodexIosPlatformCallbacks;

void codex_ios_platform_set_callbacks(
    const CodexIosPlatformCallbacks *callbacks);

void codex_ios_platform_clear_callbacks(void);

void codex_ios_platform_process_stdout(
    CodexIosHandle handle,
    const uint8_t *bytes,
    size_t len);

void codex_ios_platform_process_stderr(
    CodexIosHandle handle,
    const uint8_t *bytes,
    size_t len);

void codex_ios_platform_process_exited(
    CodexIosHandle handle,
    int32_t exit_code);

void codex_ios_platform_process_failed(
    CodexIosHandle handle,
    const char *message);

uint64_t codex_app_server_ios_start_with_stdio_fds(
    int input_read_fd,
    int output_write_fd);

int32_t codex_app_server_ios_request_stop(uint64_t handle_id);

int32_t codex_app_server_ios_stop(uint64_t handle_id);

#ifdef __cplusplus
}
#endif

#endif

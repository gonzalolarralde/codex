//! iOS embedding callbacks for platform capabilities that Codex normally gets
//! from the host OS.
//!
//! This crate intentionally sits below app-server so lower-level crates can
//! route iOS-only operations through the embedding app. Callback slots are
//! typed by capability, while the legacy unsupported-operation callback remains
//! as a fallback for user-visible errors.

use std::collections::HashMap;
use std::ffi::CStr;
use std::ffi::CString;
use std::fmt;
use std::os::raw::c_char;
use std::os::raw::c_void;
use std::ptr;
use std::sync::OnceLock;
use std::sync::RwLock;
#[cfg(target_os = "ios")]
use std::sync::Mutex;
#[cfg(target_os = "ios")]
use std::sync::atomic::AtomicU64;
#[cfg(target_os = "ios")]
use std::sync::atomic::Ordering;

#[cfg(target_os = "ios")]
use tokio::sync::broadcast;
#[cfg(target_os = "ios")]
use tokio::sync::oneshot;

/// ABI version for [`CodexIosPlatformCallbacks`].
pub const CODEX_IOS_PLATFORM_CALLBACKS_VERSION: u32 = 3;

/// Callback operation for app-server `command/exec` requests.
pub const OPERATION_COMMAND_EXEC: &str = "command.exec";
/// Callback operation for local process execution.
pub const OPERATION_PROCESS_SPAWN: &str = "process.spawn";
/// Callback operation for internal git command execution.
pub const OPERATION_GIT_COMMAND: &str = "git.command";
/// Callback operation for git apply helpers.
pub const OPERATION_GIT_APPLY: &str = "git.apply";
/// Callback operation for git staging helpers.
pub const OPERATION_GIT_STAGE: &str = "git.stage";
/// Callback operation for shell snapshot generation.
pub const OPERATION_SHELL_SNAPSHOT: &str = "shell.snapshot";
/// Callback operation for hook command execution.
pub const OPERATION_HOOK_COMMAND: &str = "hook.command";
/// Callback operation for local stdio MCP server launch.
pub const OPERATION_MCP_STDIO: &str = "mcp.stdio";
/// Callback operation for doctor-report subprocess collection.
pub const OPERATION_FEEDBACK_DOCTOR_REPORT: &str = "feedback.doctorReport";
/// Callback operation for the code-mode JavaScript runtime.
pub const OPERATION_JAVASCRIPT_RUNTIME: &str = "javascript.runtime";

pub type CodexIosRequestId = u64;
pub type CodexIosHandle = u64;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CodexIosString {
    pub ptr: *const c_char,
    pub len: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CodexIosStringArray {
    pub values: *const CodexIosString,
    pub len: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CodexIosEnvVar {
    pub key: CodexIosString,
    pub value: CodexIosString,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CodexIosEnvVarArray {
    pub values: *const CodexIosEnvVar,
    pub len: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CodexIosErrorBuffer {
    pub message_buffer: *mut c_char,
    pub message_buffer_len: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodexIosStatus {
    Accepted = 0,
    Unsupported = 1,
    Error = 2,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodexIosProcessPurpose {
    CommandExec = 0,
    ProcessSpawn = 1,
    ShellTool = 2,
    FsSandbox = 3,
    HookCommand = 4,
    McpStdio = 5,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CodexIosTerminalSize {
    pub cols: u16,
    pub rows: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CodexIosProcessSpawnRequest {
    pub request_id: CodexIosRequestId,
    pub purpose: CodexIosProcessPurpose,
    pub program: CodexIosString,
    pub args: CodexIosStringArray,
    pub cwd: CodexIosString,
    pub env: CodexIosEnvVarArray,
    pub arg0: CodexIosString,
    pub tty: bool,
    pub stream_stdin: bool,
    pub size: CodexIosTerminalSize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CodexIosGitRunRequest {
    pub cwd: CodexIosString,
    pub program: CodexIosString,
    pub args: CodexIosStringArray,
    pub env: CodexIosEnvVarArray,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CodexIosGitApplyRequest {
    pub cwd: CodexIosString,
    pub git_config: CodexIosStringArray,
    pub args: CodexIosStringArray,
    pub diff: CodexIosString,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CodexIosGitStageRequest {
    pub git_root: CodexIosString,
    pub diff: CodexIosString,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CodexIosShellSnapshotRequest {
    pub shell: CodexIosString,
    pub script: CodexIosString,
    pub cwd: CodexIosString,
    pub login_shell: bool,
    pub timeout_ms: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CodexIosMcpStdioRequest {
    pub program: CodexIosString,
    pub args: CodexIosStringArray,
    pub cwd: CodexIosString,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CodexIosDoctorReportRequest {
    pub codex_home: CodexIosString,
}

pub type UnsupportedOperationCallback = unsafe extern "C" fn(
    context: *mut c_void,
    operation: *const c_char,
    payload_json: *const c_char,
    message_buffer: *mut c_char,
    message_buffer_len: usize,
) -> usize;

pub type LogMessageCallback = unsafe extern "C" fn(
    context: *mut c_void,
    level: *const c_char,
    target: *const c_char,
    message: *const c_char,
);

pub type ProcessSpawnCallback = unsafe extern "C" fn(
    context: *mut c_void,
    request: *const CodexIosProcessSpawnRequest,
    error: CodexIosErrorBuffer,
) -> CodexIosStatus;

pub type ProcessWriteStdinCallback = unsafe extern "C" fn(
    context: *mut c_void,
    handle: CodexIosHandle,
    bytes: *const u8,
    len: usize,
    close_stdin: bool,
    error: CodexIosErrorBuffer,
) -> CodexIosStatus;

pub type ProcessResizePtyCallback = unsafe extern "C" fn(
    context: *mut c_void,
    handle: CodexIosHandle,
    size: CodexIosTerminalSize,
    error: CodexIosErrorBuffer,
) -> CodexIosStatus;

pub type ProcessTerminateCallback = unsafe extern "C" fn(
    context: *mut c_void,
    handle: CodexIosHandle,
    error: CodexIosErrorBuffer,
) -> CodexIosStatus;

pub type CodeModeCreateSessionCallback = unsafe extern "C" fn(
    context: *mut c_void,
    request_id: CodexIosRequestId,
    response_json: CodexIosErrorBuffer,
) -> CodexIosStatus;

pub type CodeModeExecuteCallback = unsafe extern "C" fn(
    context: *mut c_void,
    session_handle: CodexIosHandle,
    request_json: CodexIosString,
    response_json: CodexIosErrorBuffer,
) -> CodexIosStatus;

pub type CodeModeWaitCallback = unsafe extern "C" fn(
    context: *mut c_void,
    session_handle: CodexIosHandle,
    cell_id: CodexIosString,
    yield_time_ms: u64,
    response_json: CodexIosErrorBuffer,
) -> CodexIosStatus;

pub type CodeModeTerminateCallback = unsafe extern "C" fn(
    context: *mut c_void,
    session_handle: CodexIosHandle,
    cell_id: CodexIosString,
    response_json: CodexIosErrorBuffer,
) -> CodexIosStatus;

pub type CodeModeShutdownCallback = unsafe extern "C" fn(
    context: *mut c_void,
    session_handle: CodexIosHandle,
    error: CodexIosErrorBuffer,
) -> CodexIosStatus;

pub type GitRunCallback = unsafe extern "C" fn(
    context: *mut c_void,
    request: *const CodexIosGitRunRequest,
    response_json: CodexIosErrorBuffer,
) -> CodexIosStatus;

pub type GitApplyCallback = unsafe extern "C" fn(
    context: *mut c_void,
    request: *const CodexIosGitApplyRequest,
    response_json: CodexIosErrorBuffer,
) -> CodexIosStatus;

pub type GitStageCallback = unsafe extern "C" fn(
    context: *mut c_void,
    request: *const CodexIosGitStageRequest,
    error: CodexIosErrorBuffer,
) -> CodexIosStatus;

pub type ShellSnapshotCallback = unsafe extern "C" fn(
    context: *mut c_void,
    request: *const CodexIosShellSnapshotRequest,
    response: CodexIosErrorBuffer,
) -> CodexIosStatus;

pub type McpStdioLaunchCallback = unsafe extern "C" fn(
    context: *mut c_void,
    request: *const CodexIosMcpStdioRequest,
    error: CodexIosErrorBuffer,
) -> CodexIosStatus;

pub type DoctorReportCallback = unsafe extern "C" fn(
    context: *mut c_void,
    request: *const CodexIosDoctorReportRequest,
    response_json: CodexIosErrorBuffer,
) -> CodexIosStatus;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CodexIosPlatformCallbacks {
    pub version: u32,
    pub struct_size: usize,
    pub context: *mut c_void,
    pub unsupported_operation: Option<UnsupportedOperationCallback>,
    pub log_message: Option<LogMessageCallback>,
    pub process_spawn: Option<ProcessSpawnCallback>,
    pub process_write_stdin: Option<ProcessWriteStdinCallback>,
    pub process_resize_pty: Option<ProcessResizePtyCallback>,
    pub process_terminate: Option<ProcessTerminateCallback>,
    pub code_mode_create_session: Option<CodeModeCreateSessionCallback>,
    pub code_mode_execute: Option<CodeModeExecuteCallback>,
    pub code_mode_wait: Option<CodeModeWaitCallback>,
    pub code_mode_terminate: Option<CodeModeTerminateCallback>,
    pub code_mode_shutdown: Option<CodeModeShutdownCallback>,
    pub git_run: Option<GitRunCallback>,
    pub git_apply_patch: Option<GitApplyCallback>,
    pub git_stage_paths: Option<GitStageCallback>,
    pub shell_snapshot: Option<ShellSnapshotCallback>,
    pub mcp_stdio_launch: Option<McpStdioLaunchCallback>,
    pub doctor_report: Option<DoctorReportCallback>,
}

unsafe impl Send for CodexIosPlatformCallbacks {}
unsafe impl Sync for CodexIosPlatformCallbacks {}

static CALLBACKS: OnceLock<RwLock<Option<CodexIosPlatformCallbacks>>> = OnceLock::new();

fn callbacks() -> &'static RwLock<Option<CodexIosPlatformCallbacks>> {
    CALLBACKS.get_or_init(|| RwLock::new(None))
}

fn current_callbacks() -> Option<CodexIosPlatformCallbacks> {
    callbacks()
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .copied()
}

pub fn set_callbacks(callbacks_to_set: Option<CodexIosPlatformCallbacks>) {
    let mut callbacks = callbacks()
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *callbacks = callbacks_to_set.filter(|callbacks| {
        callbacks.version == CODEX_IOS_PLATFORM_CALLBACKS_VERSION
            && callbacks.struct_size >= std::mem::size_of::<CodexIosPlatformCallbacks>()
    });
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum IosPlatformError {
    Unsupported(String),
    Error(String),
}

impl fmt::Display for IosPlatformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(message) | Self::Error(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for IosPlatformError {}

pub fn generic_unsupported_message(operation: &str) -> String {
    format!("iOS platform operation `{operation}` is unsupported")
}

pub fn unsupported_message(operation: &str, payload_json: &str) -> Option<String> {
    let callbacks = current_callbacks()?;
    let callback = callbacks.unsupported_operation?;

    let Ok(operation) = CString::new(operation) else {
        return None;
    };
    let Ok(payload_json) = CString::new(payload_json) else {
        return None;
    };

    let mut message_buffer = vec![0_u8; 4096];
    let written = unsafe {
        callback(
            callbacks.context,
            operation.as_ptr(),
            payload_json.as_ptr(),
            message_buffer.as_mut_ptr().cast::<c_char>(),
            message_buffer.len(),
        )
    };
    if written == 0 {
        return None;
    }

    let terminator = written.min(message_buffer.len().saturating_sub(1));
    message_buffer[terminator] = 0;
    let message = unsafe { CStr::from_ptr(message_buffer.as_ptr().cast::<c_char>()) };
    Some(message.to_string_lossy().into_owned())
}

pub fn unsupported_error(operation: &str, payload_json: &str) -> IosPlatformError {
    IosPlatformError::Unsupported(
        unsupported_message(operation, payload_json)
            .unwrap_or_else(|| generic_unsupported_message(operation)),
    )
}

pub fn log_message(level: &str, target: &str, message: &str) {
    let Some(callbacks) = current_callbacks() else {
        return;
    };
    let Some(callback) = callbacks.log_message else {
        return;
    };

    let Ok(level) = CString::new(level) else {
        return;
    };
    let Ok(target) = CString::new(target) else {
        return;
    };
    let Ok(message) = CString::new(message) else {
        return;
    };

    unsafe {
        callback(
            callbacks.context,
            level.as_ptr(),
            target.as_ptr(),
            message.as_ptr(),
        );
    }
}

pub fn string_payload(fields: &[(&str, String)]) -> String {
    let mut payload = String::from("{");
    for (index, (key, value)) in fields.iter().enumerate() {
        if index > 0 {
            payload.push(',');
        }
        payload.push('"');
        push_json_string_content(&mut payload, key);
        payload.push_str("\":\"");
        push_json_string_content(&mut payload, value);
        payload.push('"');
    }
    payload.push('}');
    payload
}

fn push_json_string_content(output: &mut String, input: &str) {
    for ch in input.chars() {
        match ch {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            '\u{08}' => output.push_str("\\b"),
            '\u{0c}' => output.push_str("\\f"),
            ch if ch.is_control() => {
                output.push_str("\\u");
                output.push_str(&format!("{:04x}", ch as u32));
            }
            ch => output.push(ch),
        }
    }
}

#[cfg(test)]
fn write_buffer(buffer: CodexIosErrorBuffer, message: &str) -> usize {
    if buffer.message_buffer.is_null() || buffer.message_buffer_len == 0 {
        return 0;
    }
    let bytes = message.as_bytes();
    let len = bytes.len().min(buffer.message_buffer_len.saturating_sub(1));
    unsafe {
        ptr::copy_nonoverlapping(bytes.as_ptr(), buffer.message_buffer.cast::<u8>(), len);
        *buffer.message_buffer.add(len) = 0;
    }
    len
}

fn read_buffer(buffer: &[u8]) -> String {
    let len = buffer.iter().position(|byte| *byte == 0).unwrap_or(buffer.len());
    String::from_utf8_lossy(&buffer[..len]).into_owned()
}

fn call_with_output<F>(
    operation: &str,
    payload_json: &str,
    callback: F,
) -> Result<String, IosPlatformError>
where
    F: FnOnce(CodexIosErrorBuffer) -> CodexIosStatus,
{
    let mut output = vec![0_u8; 64 * 1024];
    let buffer = CodexIosErrorBuffer {
        message_buffer: output.as_mut_ptr().cast::<c_char>(),
        message_buffer_len: output.len(),
    };
    match callback(buffer) {
        CodexIosStatus::Accepted => Ok(read_buffer(&output)),
        CodexIosStatus::Unsupported => Err(unsupported_error(operation, payload_json)),
        CodexIosStatus::Error => {
            let message = read_buffer(&output);
            Err(IosPlatformError::Error(if message.is_empty() {
                generic_unsupported_message(operation)
            } else {
                message
            }))
        }
    }
}

struct StringArena {
    strings: Vec<CString>,
}

impl StringArena {
    fn new() -> Self {
        Self {
            strings: Vec::new(),
        }
    }

    fn push(&mut self, value: &str) -> CodexIosString {
        match CString::new(value) {
            Ok(value) => {
                let len = value.as_bytes().len();
                self.strings.push(value);
                CodexIosString {
                    ptr: self.strings.last().map_or(ptr::null(), |value| value.as_ptr()),
                    len,
                }
            }
            Err(_) => CodexIosString {
                ptr: ptr::null(),
                len: 0,
            },
        }
    }
}

fn string_array(arena: &mut StringArena, values: &[String]) -> (Vec<CodexIosString>, CodexIosStringArray) {
    let c_values = values
        .iter()
        .map(|value| arena.push(value))
        .collect::<Vec<_>>();
    let array = CodexIosStringArray {
        values: c_values.as_ptr(),
        len: c_values.len(),
    };
    (c_values, array)
}

fn env_array(
    arena: &mut StringArena,
    values: &HashMap<String, String>,
) -> (Vec<CodexIosEnvVar>, CodexIosEnvVarArray) {
    let c_values = values
        .iter()
        .map(|(key, value)| CodexIosEnvVar {
            key: arena.push(key),
            value: arena.push(value),
        })
        .collect::<Vec<_>>();
    let array = CodexIosEnvVarArray {
        values: c_values.as_ptr(),
        len: c_values.len(),
    };
    (c_values, array)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessPurpose {
    CommandExec,
    ProcessSpawn,
    ShellTool,
    FsSandbox,
    HookCommand,
    McpStdio,
}

#[cfg(target_os = "ios")]
impl ProcessPurpose {
    fn ffi(self) -> CodexIosProcessPurpose {
        match self {
            Self::CommandExec => CodexIosProcessPurpose::CommandExec,
            Self::ProcessSpawn => CodexIosProcessPurpose::ProcessSpawn,
            Self::ShellTool => CodexIosProcessPurpose::ShellTool,
            Self::FsSandbox => CodexIosProcessPurpose::FsSandbox,
            Self::HookCommand => CodexIosProcessPurpose::HookCommand,
            Self::McpStdio => CodexIosProcessPurpose::McpStdio,
        }
    }
}

pub struct ProcessSpawnConfig<'a> {
    pub purpose: ProcessPurpose,
    pub program: &'a str,
    pub args: &'a [String],
    pub cwd: &'a str,
    pub env: &'a HashMap<String, String>,
    pub arg0: Option<&'a str>,
    pub tty: bool,
    pub stream_stdin: bool,
    pub size: TerminalSize,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TerminalSize {
    pub cols: u16,
    pub rows: u16,
}

#[cfg(target_os = "ios")]
pub struct IosProcess {
    pub handle: CodexIosHandle,
    pub stdout_rx: broadcast::Receiver<Vec<u8>>,
    pub stderr_rx: broadcast::Receiver<Vec<u8>>,
    pub exit_rx: oneshot::Receiver<i32>,
}

#[cfg(target_os = "ios")]
struct PendingProcess {
    stdout_tx: broadcast::Sender<Vec<u8>>,
    stderr_tx: broadcast::Sender<Vec<u8>>,
    exit_tx: Option<oneshot::Sender<i32>>,
}

#[cfg(target_os = "ios")]
static PROCESS_REGISTRY: OnceLock<Mutex<HashMap<CodexIosHandle, PendingProcess>>> = OnceLock::new();
#[cfg(target_os = "ios")]
static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

#[cfg(target_os = "ios")]
fn process_registry() -> &'static Mutex<HashMap<CodexIosHandle, PendingProcess>> {
    PROCESS_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(target_os = "ios")]
pub fn process_spawn(config: ProcessSpawnConfig<'_>) -> Result<IosProcess, IosPlatformError> {
    let callbacks = current_callbacks()
        .ok_or_else(|| unsupported_error(OPERATION_PROCESS_SPAWN, "{}"))?;
    let callback = callbacks
        .process_spawn
        .ok_or_else(|| unsupported_error(OPERATION_PROCESS_SPAWN, "{}"))?;
    let handle = NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    let (stdout_tx, stdout_rx) = broadcast::channel(256);
    let (stderr_tx, stderr_rx) = broadcast::channel(256);
    let (exit_tx, exit_rx) = oneshot::channel();
    process_registry()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(
            handle,
            PendingProcess {
                stdout_tx,
                stderr_tx,
                exit_tx: Some(exit_tx),
            },
        );

    let mut arena = StringArena::new();
    let (args_values, args) = string_array(&mut arena, config.args);
    let (env_values, env) = env_array(&mut arena, config.env);
    let request = CodexIosProcessSpawnRequest {
        request_id: handle,
        purpose: config.purpose.ffi(),
        program: arena.push(config.program),
        args,
        cwd: arena.push(config.cwd),
        env,
        arg0: arena.push(config.arg0.unwrap_or("")),
        tty: config.tty,
        stream_stdin: config.stream_stdin,
        size: CodexIosTerminalSize {
            cols: config.size.cols,
            rows: config.size.rows,
        },
    };
    let _keep_alive = (args_values, env_values);
    let mut error = vec![0_u8; 4096];
    let status = unsafe {
        callback(
            callbacks.context,
            &request,
            CodexIosErrorBuffer {
                message_buffer: error.as_mut_ptr().cast::<c_char>(),
                message_buffer_len: error.len(),
            },
        )
    };
    match status {
        CodexIosStatus::Accepted => Ok(IosProcess {
            handle,
            stdout_rx,
            stderr_rx,
            exit_rx,
        }),
        CodexIosStatus::Unsupported => {
            process_registry()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&handle);
            Err(unsupported_error(OPERATION_PROCESS_SPAWN, "{}"))
        }
        CodexIosStatus::Error => {
            process_registry()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&handle);
            let message = read_buffer(&error);
            Err(IosPlatformError::Error(if message.is_empty() {
                generic_unsupported_message(OPERATION_PROCESS_SPAWN)
            } else {
                message
            }))
        }
    }
}

#[cfg(target_os = "ios")]
pub fn process_write_stdin(
    handle: CodexIosHandle,
    bytes: &[u8],
    close_stdin: bool,
) -> Result<(), IosPlatformError> {
    let callbacks = current_callbacks()
        .ok_or_else(|| unsupported_error(OPERATION_PROCESS_SPAWN, "{}"))?;
    let callback = callbacks
        .process_write_stdin
        .ok_or_else(|| unsupported_error(OPERATION_PROCESS_SPAWN, "{}"))?;
    status_to_result(OPERATION_PROCESS_SPAWN, "{}", |buffer| unsafe {
        callback(
            callbacks.context,
            handle,
            bytes.as_ptr(),
            bytes.len(),
            close_stdin,
            buffer,
        )
    })
}

#[cfg(target_os = "ios")]
pub fn process_resize_pty(
    handle: CodexIosHandle,
    size: TerminalSize,
) -> Result<(), IosPlatformError> {
    let callbacks = current_callbacks()
        .ok_or_else(|| unsupported_error(OPERATION_PROCESS_SPAWN, "{}"))?;
    let callback = callbacks
        .process_resize_pty
        .ok_or_else(|| unsupported_error(OPERATION_PROCESS_SPAWN, "{}"))?;
    status_to_result(OPERATION_PROCESS_SPAWN, "{}", |buffer| unsafe {
        callback(
            callbacks.context,
            handle,
            CodexIosTerminalSize {
                cols: size.cols,
                rows: size.rows,
            },
            buffer,
        )
    })
}

#[cfg(target_os = "ios")]
pub fn process_terminate(handle: CodexIosHandle) -> Result<(), IosPlatformError> {
    let callbacks = current_callbacks()
        .ok_or_else(|| unsupported_error(OPERATION_PROCESS_SPAWN, "{}"))?;
    let callback = callbacks
        .process_terminate
        .ok_or_else(|| unsupported_error(OPERATION_PROCESS_SPAWN, "{}"))?;
    status_to_result(OPERATION_PROCESS_SPAWN, "{}", |buffer| unsafe {
        callback(callbacks.context, handle, buffer)
    })
}

fn status_to_result<F>(operation: &str, payload_json: &str, callback: F) -> Result<(), IosPlatformError>
where
    F: FnOnce(CodexIosErrorBuffer) -> CodexIosStatus,
{
    call_with_output(operation, payload_json, callback).map(|_| ())
}

pub fn code_mode_execute(request_json: &str) -> Result<String, IosPlatformError> {
    let callbacks = current_callbacks()
        .ok_or_else(|| unsupported_error(OPERATION_JAVASCRIPT_RUNTIME, request_json))?;
    let callback = callbacks
        .code_mode_execute
        .ok_or_else(|| unsupported_error(OPERATION_JAVASCRIPT_RUNTIME, request_json))?;
    let mut arena = StringArena::new();
    let request = arena.push(request_json);
    call_with_output(OPERATION_JAVASCRIPT_RUNTIME, request_json, |buffer| unsafe {
        callback(callbacks.context, 0, request, buffer)
    })
}

pub fn code_mode_wait(cell_id: &str, yield_time_ms: u64) -> Result<String, IosPlatformError> {
    let callbacks = current_callbacks()
        .ok_or_else(|| unsupported_error(OPERATION_JAVASCRIPT_RUNTIME, "{}"))?;
    let callback = callbacks
        .code_mode_wait
        .ok_or_else(|| unsupported_error(OPERATION_JAVASCRIPT_RUNTIME, "{}"))?;
    let mut arena = StringArena::new();
    let cell_id = arena.push(cell_id);
    call_with_output(OPERATION_JAVASCRIPT_RUNTIME, "{}", |buffer| unsafe {
        callback(callbacks.context, 0, cell_id, yield_time_ms, buffer)
    })
}

pub fn code_mode_terminate(cell_id: &str) -> Result<String, IosPlatformError> {
    let callbacks = current_callbacks()
        .ok_or_else(|| unsupported_error(OPERATION_JAVASCRIPT_RUNTIME, "{}"))?;
    let callback = callbacks
        .code_mode_terminate
        .ok_or_else(|| unsupported_error(OPERATION_JAVASCRIPT_RUNTIME, "{}"))?;
    let mut arena = StringArena::new();
    let cell_id = arena.push(cell_id);
    call_with_output(OPERATION_JAVASCRIPT_RUNTIME, "{}", |buffer| unsafe {
        callback(callbacks.context, 0, cell_id, buffer)
    })
}

pub fn shell_snapshot(
    shell: &str,
    script: &str,
    cwd: &str,
    login_shell: bool,
    timeout_ms: u64,
) -> Result<String, IosPlatformError> {
    let payload = string_payload(&[
        ("shell", shell.to_string()),
        ("script", script.to_string()),
        ("cwd", cwd.to_string()),
    ]);
    let callbacks = current_callbacks().ok_or_else(|| unsupported_error(OPERATION_SHELL_SNAPSHOT, &payload))?;
    let callback = callbacks
        .shell_snapshot
        .ok_or_else(|| unsupported_error(OPERATION_SHELL_SNAPSHOT, &payload))?;
    let mut arena = StringArena::new();
    let request = CodexIosShellSnapshotRequest {
        shell: arena.push(shell),
        script: arena.push(script),
        cwd: arena.push(cwd),
        login_shell,
        timeout_ms,
    };
    call_with_output(OPERATION_SHELL_SNAPSHOT, &payload, |buffer| unsafe {
        callback(callbacks.context, &request, buffer)
    })
}

pub fn git_run(
    cwd: &str,
    program: &str,
    args: &[String],
    env: &HashMap<String, String>,
) -> Result<String, IosPlatformError> {
    let payload = string_payload(&[
        ("command", format!("{} {}", program, args.join(" "))),
        ("cwd", cwd.to_string()),
    ]);
    let callbacks = current_callbacks().ok_or_else(|| unsupported_error(OPERATION_GIT_COMMAND, &payload))?;
    let callback = callbacks
        .git_run
        .ok_or_else(|| unsupported_error(OPERATION_GIT_COMMAND, &payload))?;
    let mut arena = StringArena::new();
    let (args_values, args) = string_array(&mut arena, args);
    let (env_values, env) = env_array(&mut arena, env);
    let request = CodexIosGitRunRequest {
        cwd: arena.push(cwd),
        program: arena.push(program),
        args,
        env,
    };
    let _keep_alive = (args_values, env_values);
    call_with_output(OPERATION_GIT_COMMAND, &payload, |buffer| unsafe {
        callback(callbacks.context, &request, buffer)
    })
}

pub fn git_apply(
    cwd: &str,
    git_config: &[String],
    args: &[String],
    diff: &str,
) -> Result<String, IosPlatformError> {
    let payload = string_payload(&[
        ("command", format!("git {}", args.join(" "))),
        ("cwd", cwd.to_string()),
    ]);
    let callbacks = current_callbacks().ok_or_else(|| unsupported_error(OPERATION_GIT_APPLY, &payload))?;
    let callback = callbacks
        .git_apply_patch
        .ok_or_else(|| unsupported_error(OPERATION_GIT_APPLY, &payload))?;
    let mut arena = StringArena::new();
    let (config_values, git_config) = string_array(&mut arena, git_config);
    let (args_values, args) = string_array(&mut arena, args);
    let request = CodexIosGitApplyRequest {
        cwd: arena.push(cwd),
        git_config,
        args,
        diff: arena.push(diff),
    };
    let _keep_alive = (config_values, args_values);
    call_with_output(OPERATION_GIT_APPLY, &payload, |buffer| unsafe {
        callback(callbacks.context, &request, buffer)
    })
}

pub fn git_stage(git_root: &str, diff: &str) -> Result<(), IosPlatformError> {
    let payload = string_payload(&[
        ("gitRoot", git_root.to_string()),
        ("diffBytes", diff.len().to_string()),
    ]);
    let callbacks = current_callbacks().ok_or_else(|| unsupported_error(OPERATION_GIT_STAGE, &payload))?;
    let callback = callbacks
        .git_stage_paths
        .ok_or_else(|| unsupported_error(OPERATION_GIT_STAGE, &payload))?;
    let mut arena = StringArena::new();
    let request = CodexIosGitStageRequest {
        git_root: arena.push(git_root),
        diff: arena.push(diff),
    };
    status_to_result(OPERATION_GIT_STAGE, &payload, |buffer| unsafe {
        callback(callbacks.context, &request, buffer)
    })
}

pub fn mcp_stdio_launch(program: &str, args: &[String], cwd: &str) -> Result<(), IosPlatformError> {
    let payload = string_payload(&[
        ("program", program.to_string()),
        ("args", args.join(" ")),
        ("cwd", cwd.to_string()),
    ]);
    let callbacks = current_callbacks().ok_or_else(|| unsupported_error(OPERATION_MCP_STDIO, &payload))?;
    let callback = callbacks
        .mcp_stdio_launch
        .ok_or_else(|| unsupported_error(OPERATION_MCP_STDIO, &payload))?;
    let mut arena = StringArena::new();
    let (arg_values, args) = string_array(&mut arena, args);
    let request = CodexIosMcpStdioRequest {
        program: arena.push(program),
        args,
        cwd: arena.push(cwd),
    };
    let _keep_alive = arg_values;
    status_to_result(OPERATION_MCP_STDIO, &payload, |buffer| unsafe {
        callback(callbacks.context, &request, buffer)
    })
}

pub fn doctor_report(codex_home: &str) -> Result<Option<String>, IosPlatformError> {
    let payload = string_payload(&[("codexHome", codex_home.to_string())]);
    let callbacks = current_callbacks().ok_or_else(|| unsupported_error(OPERATION_FEEDBACK_DOCTOR_REPORT, &payload))?;
    let callback = callbacks
        .doctor_report
        .ok_or_else(|| unsupported_error(OPERATION_FEEDBACK_DOCTOR_REPORT, &payload))?;
    let mut arena = StringArena::new();
    let request = CodexIosDoctorReportRequest {
        codex_home: arena.push(codex_home),
    };
    let output = call_with_output(OPERATION_FEEDBACK_DOCTOR_REPORT, &payload, |buffer| unsafe {
        callback(callbacks.context, &request, buffer)
    })?;
    Ok((!output.is_empty()).then_some(output))
}

#[unsafe(no_mangle)]
pub extern "C" fn codex_ios_platform_process_stdout(
    handle: CodexIosHandle,
    bytes: *const u8,
    len: usize,
) {
    process_output(handle, bytes, len, true);
}

#[unsafe(no_mangle)]
pub extern "C" fn codex_ios_platform_process_stderr(
    handle: CodexIosHandle,
    bytes: *const u8,
    len: usize,
) {
    process_output(handle, bytes, len, false);
}

#[unsafe(no_mangle)]
pub extern "C" fn codex_ios_platform_process_exited(handle: CodexIosHandle, exit_code: i32) {
    #[cfg(target_os = "ios")]
    if let Some(mut process) = process_registry()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&handle)
        && let Some(exit_tx) = process.exit_tx.take()
    {
        let _ = exit_tx.send(exit_code);
    }

    #[cfg(not(target_os = "ios"))]
    let _ = (handle, exit_code);
}

#[unsafe(no_mangle)]
pub extern "C" fn codex_ios_platform_process_failed(
    handle: CodexIosHandle,
    message: *const c_char,
) {
    #[cfg(target_os = "ios")]
    {
        if let Some(process) = process_registry()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&handle)
        {
            if !message.is_null() {
                let text = unsafe { CStr::from_ptr(message) }.to_string_lossy();
                let _ = process.stderr_tx.send(text.as_bytes().to_vec());
            }
        }
        codex_ios_platform_process_exited(handle, -1);
    }

    #[cfg(not(target_os = "ios"))]
    let _ = (handle, message);
}

#[cfg(target_os = "ios")]
fn process_output(handle: CodexIosHandle, bytes: *const u8, len: usize, stdout: bool) {
    if bytes.is_null() || len == 0 {
        return;
    }
    let chunk = unsafe { std::slice::from_raw_parts(bytes, len) }.to_vec();
    if let Some(process) = process_registry()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&handle)
    {
        let _ = if stdout {
            process.stdout_tx.send(chunk)
        } else {
            process.stderr_tx.send(chunk)
        };
    }
}

#[cfg(not(target_os = "ios"))]
fn process_output(handle: CodexIosHandle, bytes: *const u8, len: usize, stdout: bool) {
    let _ = (handle, bytes, len, stdout);
}

#[unsafe(no_mangle)]
pub extern "C" fn codex_ios_platform_clear_callbacks() {
    set_callbacks(None);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn codex_ios_platform_set_callbacks(
    callbacks_to_set: *const CodexIosPlatformCallbacks,
) {
    let callbacks_to_set = if callbacks_to_set.is_null() {
        None
    } else {
        Some(unsafe { ptr::read(callbacks_to_set) })
    };
    set_callbacks(callbacks_to_set);
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::ptr;

    unsafe extern "C" fn override_message(
        _context: *mut c_void,
        operation: *const c_char,
        _payload_json: *const c_char,
        message_buffer: *mut c_char,
        message_buffer_len: usize,
    ) -> usize {
        let operation = unsafe { CStr::from_ptr(operation) }.to_string_lossy();
        write_buffer(
            CodexIosErrorBuffer {
                message_buffer,
                message_buffer_len,
            },
            &format!("swift handled {operation}"),
        )
    }

    unsafe extern "C" fn shell_snapshot(
        _context: *mut c_void,
        _request: *const CodexIosShellSnapshotRequest,
        response: CodexIosErrorBuffer,
    ) -> CodexIosStatus {
        write_buffer(response, "snapshot");
        CodexIosStatus::Accepted
    }

    fn test_callbacks() -> CodexIosPlatformCallbacks {
        CodexIosPlatformCallbacks {
            version: CODEX_IOS_PLATFORM_CALLBACKS_VERSION,
            struct_size: std::mem::size_of::<CodexIosPlatformCallbacks>(),
            context: ptr::null_mut(),
            unsupported_operation: None,
            log_message: None,
            process_spawn: None,
            process_write_stdin: None,
            process_resize_pty: None,
            process_terminate: None,
            code_mode_create_session: None,
            code_mode_execute: None,
            code_mode_wait: None,
            code_mode_terminate: None,
            code_mode_shutdown: None,
            git_run: None,
            git_apply_patch: None,
            git_stage_paths: None,
            shell_snapshot: None,
            mcp_stdio_launch: None,
            doctor_report: None,
        }
    }

    #[test]
    fn unsupported_message_returns_none_without_callback() {
        set_callbacks(None);

        assert_eq!(unsupported_message(OPERATION_PROCESS_SPAWN, "{}"), None);
    }

    #[test]
    fn unsupported_message_uses_callback_override() {
        let mut callbacks = test_callbacks();
        callbacks.unsupported_operation = Some(override_message);
        set_callbacks(Some(callbacks));

        assert_eq!(
            unsupported_message(OPERATION_PROCESS_SPAWN, "{}"),
            Some("swift handled process.spawn".to_string())
        );

        set_callbacks(None);
    }

    #[test]
    fn typed_shell_snapshot_callback_returns_output() {
        let mut callbacks = test_callbacks();
        callbacks.shell_snapshot = Some(shell_snapshot);
        set_callbacks(Some(callbacks));

        assert_eq!(
            super::shell_snapshot("zsh", "echo ok", "/tmp", true, 1000),
            Ok("snapshot".to_string())
        );

        set_callbacks(None);
    }

    #[test]
    fn typed_callback_falls_back_to_unsupported_message() {
        let mut callbacks = test_callbacks();
        callbacks.unsupported_operation = Some(override_message);
        set_callbacks(Some(callbacks));

        assert_eq!(
            super::shell_snapshot("zsh", "echo ok", "/tmp", true, 1000),
            Err(IosPlatformError::Unsupported(
                "swift handled shell.snapshot".to_string()
            ))
        );

        set_callbacks(None);
    }
}

//! iOS embedding callbacks for platform capabilities that Codex normally gets
//! from the host OS.
//!
//! This crate intentionally sits below app-server so lower-level crates can
//! route iOS-only unsupported operations through the embedding app. Swift may
//! register callbacks to observe unsupported operations, provide user-visible
//! messages, or eventually route operations to replacement implementations.

use std::ffi::CStr;
use std::ffi::CString;
use std::os::raw::c_char;
use std::os::raw::c_void;
use std::ptr;
use std::sync::OnceLock;
use std::sync::RwLock;

/// ABI version for [`CodexIosPlatformCallbacks`].
pub const CODEX_IOS_PLATFORM_CALLBACKS_VERSION: u32 = 1;

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
/// Callback operation reserved for a future JavaScriptCore-backed code-mode runtime.
pub const OPERATION_JAVASCRIPT_RUNTIME: &str = "javascript.runtime";

/// Callback used when an iOS platform operation reaches an unsupported iOS path.
///
/// `operation` and `payload_json` are UTF-8 C strings owned by Rust and valid
/// only for the duration of the call. Implementations may write a UTF-8 message
/// into `message_buffer` and return the number of bytes written, excluding the
/// trailing nul. Returning 0 means the operation was not handled.
pub type UnsupportedOperationCallback = unsafe extern "C" fn(
    context: *mut c_void,
    operation: *const c_char,
    payload_json: *const c_char,
    message_buffer: *mut c_char,
    message_buffer_len: usize,
) -> usize;

/// Optional callbacks registered by the Swift embedding layer.
///
/// This is a first-pass surface: callbacks can observe unsupported operations
/// and provide user-visible errors. Future versions can add operation callbacks
/// that return structured replacements, such as process streams or a
/// JavaScriptCore code-mode runtime.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CodexIosPlatformCallbacks {
    pub version: u32,
    pub context: *mut c_void,
    pub unsupported_operation: Option<UnsupportedOperationCallback>,
}

unsafe impl Send for CodexIosPlatformCallbacks {}
unsafe impl Sync for CodexIosPlatformCallbacks {}

static CALLBACKS: OnceLock<RwLock<Option<CodexIosPlatformCallbacks>>> = OnceLock::new();

fn callbacks() -> &'static RwLock<Option<CodexIosPlatformCallbacks>> {
    CALLBACKS.get_or_init(|| RwLock::new(None))
}

/// Registers callbacks for iOS platform operations.
pub fn set_callbacks(callbacks_to_set: Option<CodexIosPlatformCallbacks>) {
    let mut callbacks = callbacks()
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *callbacks = callbacks_to_set
        .filter(|callbacks| callbacks.version == CODEX_IOS_PLATFORM_CALLBACKS_VERSION);
}

/// Returns a Swift-provided message for an unsupported operation.
pub fn unsupported_message(operation: &str, payload_json: &str) -> Option<String> {
    let callbacks = callbacks()
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let callbacks = (*callbacks)?;
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

/// Generic Rust fallback used only when the embedding layer has not registered
/// an unsupported-operation message.
pub fn generic_unsupported_message(operation: &str) -> String {
    format!("iOS platform operation `{operation}` is unsupported")
}

/// Builds a small JSON object payload from string fields for callback context.
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

/// Clears registered callbacks.
#[unsafe(no_mangle)]
pub extern "C" fn codex_ios_platform_clear_callbacks() {
    set_callbacks(None);
}

/// Registers iOS platform callbacks.
///
/// Pass null to clear the current callbacks. The callback table is copied; the
/// `context` pointer remains owned by the Swift embedding layer.
///
/// # Safety
///
/// If `callbacks_to_set` is non-null, it must point to a valid
/// [`CodexIosPlatformCallbacks`] value for the duration of this call.
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

    unsafe extern "C" fn override_message(
        _context: *mut c_void,
        operation: *const c_char,
        _payload_json: *const c_char,
        message_buffer: *mut c_char,
        message_buffer_len: usize,
    ) -> usize {
        let operation = unsafe { CStr::from_ptr(operation) }.to_string_lossy();
        let message = format!("swift handled {operation}");
        let bytes = message.as_bytes();
        let len = bytes.len().min(message_buffer_len.saturating_sub(1));
        unsafe {
            ptr::copy_nonoverlapping(bytes.as_ptr(), message_buffer.cast::<u8>(), len);
            *message_buffer.add(len) = 0;
        }
        len
    }

    #[test]
    fn unsupported_message_returns_none_without_callback() {
        set_callbacks(None);

        assert_eq!(unsupported_message(OPERATION_PROCESS_SPAWN, "{}"), None);
    }

    #[test]
    fn unsupported_message_uses_callback_override() {
        set_callbacks(Some(CodexIosPlatformCallbacks {
            version: CODEX_IOS_PLATFORM_CALLBACKS_VERSION,
            context: ptr::null_mut(),
            unsupported_operation: Some(override_message),
        }));

        assert_eq!(
            unsupported_message(OPERATION_PROCESS_SPAWN, "{}"),
            Some("swift handled process.spawn".to_string())
        );

        set_callbacks(None);
    }

    #[test]
    fn string_payload_escapes_json_string_fields() {
        assert_eq!(
            string_payload(&[
                ("command", "echo \"hi\"".to_string()),
                ("line", "one\ntwo".to_string()),
            ]),
            r#"{"command":"echo \"hi\"","line":"one\ntwo"}"#
        );
    }
}

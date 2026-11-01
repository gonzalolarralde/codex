use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use codex_code_mode_protocol::CellId;
use codex_code_mode_protocol::CodeModeNestedToolCall;
use codex_code_mode_protocol::CodeModeSession;
use codex_code_mode_protocol::CodeModeSessionDelegate;
use codex_code_mode_protocol::CodeModeSessionProvider;
use codex_code_mode_protocol::CodeModeSessionProviderFuture;
use codex_code_mode_protocol::CodeModeSessionResultFuture;
use codex_code_mode_protocol::ExecuteRequest;
use codex_code_mode_protocol::RuntimeResponse;
use codex_code_mode_protocol::StartedCell;
use codex_code_mode_protocol::WaitOutcome;
use codex_code_mode_protocol::WaitRequest;
use codex_ios_platform::CodexIosErrorBuffer;
use codex_ios_platform::CodexIosHandle;
use codex_ios_platform::CodexIosStatus;
use codex_ios_platform::CodexIosString;
use serde_json::json;
use tokio::runtime::Handle;
use tokio_util::sync::CancellationToken;

#[path = "ios_observation.rs"]
mod observation;

static NEXT_SESSION_HANDLE: AtomicU64 = AtomicU64::new(/*v*/ 1);
static SESSION_BRIDGES: OnceLock<Mutex<HashMap<CodexIosHandle, Arc<SessionBridge>>>> =
    OnceLock::new();

fn disabled_message(action: &str, payload: serde_json::Value) -> String {
    let payload_json = serde_json::to_string(&json!({
        "action": action,
        "payload": payload,
    }))
    .unwrap_or_else(|_| "{}".to_string());
    codex_ios_platform::unsupported_error(
        codex_ios_platform::OPERATION_JAVASCRIPT_RUNTIME,
        &payload_json,
    )
    .to_string()
}

fn response_cell_id(response: &RuntimeResponse) -> CellId {
    match response {
        RuntimeResponse::Yielded { cell_id, .. }
        | RuntimeResponse::Terminated { cell_id, .. }
        | RuntimeResponse::Result { cell_id, .. } => cell_id.clone(),
    }
}

fn is_terminal(response: &RuntimeResponse) -> bool {
    matches!(
        response,
        RuntimeResponse::Terminated { .. } | RuntimeResponse::Result { .. }
    )
}

fn session_bridges() -> &'static Mutex<HashMap<CodexIosHandle, Arc<SessionBridge>>> {
    SESSION_BRIDGES.get_or_init(|| Mutex::new(HashMap::new()))
}

struct SessionBridge {
    runtime: Handle,
    cells: Mutex<HashMap<CellId, CellBridge>>,
}

#[derive(Clone)]
struct CellBridge {
    delegate: Arc<dyn CodeModeSessionDelegate>,
    token: CancellationToken,
}

impl SessionBridge {
    fn cell(&self, cell_id: &CellId) -> Result<CellBridge, String> {
        self.cells
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(cell_id)
            .cloned()
            .ok_or_else(|| format!("unknown iOS code-mode cell {cell_id}"))
    }

    fn close_cell(&self, cell_id: &CellId) {
        let cell = self
            .cells
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(cell_id);
        if let Some(cell) = cell {
            cell.token.cancel();
            cell.delegate.cell_closed(cell_id);
        }
    }

    fn shutdown(&self) {
        let cell_ids = self
            .cells
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for cell_id in cell_ids {
            self.close_cell(&cell_id);
        }
    }
}

/// Creates sessions backed by the embedding app's iOS JavaScript callbacks.
#[derive(Default)]
pub struct IosCodeModeSessionProvider;

impl CodeModeSessionProvider for IosCodeModeSessionProvider {
    fn create_session(&self) -> CodeModeSessionProviderFuture<'_> {
        Box::pin(async move { Ok(Arc::new(IosCodeModeSession::new()) as Arc<dyn CodeModeSession>) })
    }
}

struct IosCodeModeSession {
    session_handle: CodexIosHandle,
    bridge: Arc<SessionBridge>,
    initialized: Mutex<bool>,
}

impl IosCodeModeSession {
    fn new() -> Self {
        let session_handle = NEXT_SESSION_HANDLE.fetch_add(/*val*/ 1, Ordering::Relaxed);
        let bridge = Arc::new(SessionBridge {
            runtime: Handle::current(),
            cells: Mutex::new(HashMap::new()),
        });
        session_bridges()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(session_handle, bridge.clone());
        Self {
            session_handle,
            bridge,
            initialized: Mutex::new(/*t*/ false),
        }
    }

    fn ensure_initialized(&self) -> Result<(), String> {
        let mut initialized = self
            .initialized
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !*initialized {
            codex_ios_platform::code_mode_create_session(self.session_handle)
                .map_err(|error| error.to_string())?;
            *initialized = true;
        }
        Ok(())
    }

    fn finish_response(&self, response: &RuntimeResponse) {
        if is_terminal(response) {
            self.bridge.close_cell(&response_cell_id(response));
        }
    }

    pub async fn execute(
        &self,
        request: ExecuteRequest,
        delegate: Arc<dyn CodeModeSessionDelegate>,
    ) -> Result<StartedCell, String> {
        self.ensure_initialized()?;
        let request_json = serde_json::to_string(&request).map_err(|err| err.to_string())?;
        // The embedding ABI identifies cells by the original tool call ID.
        let cell_id = CellId::new(request.tool_call_id.clone());
        {
            let mut cells = self
                .bridge
                .cells
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match cells.entry(cell_id.clone()) {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(CellBridge {
                        delegate,
                        token: CancellationToken::new(),
                    });
                }
                std::collections::hash_map::Entry::Occupied(_) => {
                    return Err(format!("iOS code-mode cell {cell_id} is already running"));
                }
            }
        }
        let session_handle = self.session_handle;
        let result = tokio::task::spawn_blocking(move || {
            codex_ios_platform::code_mode_execute(session_handle, &request_json)
        })
        .await
        .map_err(|error| {
            self.bridge.close_cell(&cell_id);
            error.to_string()
        })?;
        match result {
            Ok(response_json) => {
                let response: RuntimeResponse =
                    serde_json::from_str(&response_json).map_err(|err| {
                        self.bridge.close_cell(&cell_id);
                        err.to_string()
                    })?;
                if response_cell_id(&response) != cell_id {
                    self.bridge.close_cell(&cell_id);
                    return Err("iOS code-mode callback returned a mismatched cell ID".to_string());
                }
                self.finish_response(&response);
                let cell_id = response_cell_id(&response);
                Ok(StartedCell::from_future(
                    cell_id,
                    async move { Ok(response) },
                ))
            }
            Err(error) => {
                self.bridge.close_cell(&cell_id);
                match error {
                    codex_ios_platform::IosPlatformError::Error(message) => Err(message),
                    codex_ios_platform::IosPlatformError::Unsupported(_) => Err(disabled_message(
                        "execute",
                        json!({
                            "toolCallId": request.tool_call_id,
                            "yieldTimeMs": request.yield_time_ms,
                            "maxOutputTokens": request.max_output_tokens,
                            "enabledToolCount": request.enabled_tools.len(),
                        }),
                    )),
                }
            }
        }
    }

    pub async fn wait(
        &self,
        request: WaitRequest,
        preempt: Option<CancellationToken>,
    ) -> Result<WaitOutcome, String> {
        match observation::wait(self.session_handle, request.clone(), preempt).await {
            Ok(outcome) => {
                let response = match &outcome {
                    WaitOutcome::LiveCell(response) | WaitOutcome::MissingCell(response) => {
                        response
                    }
                };
                self.finish_response(response);
                Ok(outcome)
            }
            Err(message) => Err(message),
        }
    }

    pub async fn terminate(&self, cell_id: CellId) -> Result<WaitOutcome, String> {
        if let Ok(cell) = self.bridge.cell(&cell_id) {
            cell.token.cancel();
        }
        match codex_ios_platform::code_mode_terminate(self.session_handle, cell_id.as_str()) {
            Ok(response_json) => {
                let outcome: WaitOutcome =
                    serde_json::from_str(&response_json).map_err(|err| err.to_string())?;
                self.bridge.close_cell(&cell_id);
                Ok(outcome)
            }
            Err(error) => Err(error.to_string()),
        }
    }

    pub async fn shutdown(&self) -> Result<(), String> {
        self.bridge.shutdown();
        session_bridges()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.session_handle);
        let initialized = *self
            .initialized
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if initialized {
            codex_ios_platform::code_mode_shutdown(self.session_handle)
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}

impl Drop for IosCodeModeSession {
    fn drop(&mut self) {
        self.bridge.shutdown();
        session_bridges()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.session_handle);
    }
}

impl CodeModeSession for IosCodeModeSession {
    fn execute<'a>(
        &'a self,
        request: ExecuteRequest,
        delegate: Arc<dyn CodeModeSessionDelegate>,
        _preempt: Option<CancellationToken>,
    ) -> CodeModeSessionResultFuture<'a, StartedCell> {
        // The embedding callback starts a cell and yields its initial observation
        // immediately; later observations honor preemption through wait.
        Box::pin(IosCodeModeSession::execute(self, request, delegate))
    }

    fn wait<'a>(
        &'a self,
        request: WaitRequest,
        preempt: Option<CancellationToken>,
    ) -> CodeModeSessionResultFuture<'a, WaitOutcome> {
        Box::pin(IosCodeModeSession::wait(self, request, preempt))
    }

    fn terminate<'a>(&'a self, cell_id: CellId) -> CodeModeSessionResultFuture<'a, WaitOutcome> {
        Box::pin(IosCodeModeSession::terminate(self, cell_id))
    }

    fn shutdown<'a>(&'a self) -> CodeModeSessionResultFuture<'a, ()> {
        Box::pin(IosCodeModeSession::shutdown(self))
    }
}

fn bridge_for_handle(session_handle: CodexIosHandle) -> Result<Arc<SessionBridge>, String> {
    session_bridges()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&session_handle)
        .cloned()
        .ok_or_else(|| format!("unknown iOS code-mode session {session_handle}"))
}

fn read_ffi_string(value: CodexIosString) -> Result<String, String> {
    if value.ptr.is_null() {
        return if value.len == 0 {
            Ok(String::new())
        } else {
            Err("invalid null iOS string".to_string())
        };
    }
    let bytes = unsafe { std::slice::from_raw_parts(value.ptr.cast::<u8>(), value.len) };
    String::from_utf8(bytes.to_vec()).map_err(|error| error.to_string())
}

fn write_ffi_buffer(buffer: CodexIosErrorBuffer, text: &str) -> bool {
    if buffer.message_buffer.is_null() || buffer.message_buffer_len == 0 {
        return false;
    }
    let bytes = text.as_bytes();
    if bytes.len() >= buffer.message_buffer_len {
        return false;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(
            bytes.as_ptr(),
            buffer.message_buffer.cast::<u8>(),
            bytes.len(),
        );
        *buffer.message_buffer.add(bytes.len()) = 0;
    }
    true
}

fn ffi_error(buffer: CodexIosErrorBuffer, message: impl AsRef<str>) -> CodexIosStatus {
    let message = message.as_ref();
    if !write_ffi_buffer(buffer, message) {
        let _ = write_ffi_buffer(buffer, "iOS code-mode response exceeded its buffer");
    }
    CodexIosStatus::Error
}

#[unsafe(no_mangle)]
pub extern "C" fn codex_ios_code_mode_invoke_tool(
    session_handle: CodexIosHandle,
    invocation_json: CodexIosString,
    response_json: CodexIosErrorBuffer,
) -> CodexIosStatus {
    let result = (|| {
        let bridge = bridge_for_handle(session_handle)?;
        let invocation_json = read_ffi_string(invocation_json)?;
        let invocation: CodeModeNestedToolCall =
            serde_json::from_str(&invocation_json).map_err(|error| error.to_string())?;
        let cell = bridge.cell(&invocation.cell_id)?;
        let result = bridge
            .runtime
            .block_on(cell.delegate.invoke_tool(invocation, cell.token))?;
        serde_json::to_string(&result).map_err(|error| error.to_string())
    })();

    match result {
        Ok(result_json) if write_ffi_buffer(response_json, &result_json) => {
            CodexIosStatus::Accepted
        }
        Ok(_) => ffi_error(response_json, "iOS code-mode tool response is too large"),
        Err(error) => ffi_error(response_json, error),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn codex_ios_code_mode_notify(
    session_handle: CodexIosHandle,
    call_id: CodexIosString,
    cell_id: CodexIosString,
    text: CodexIosString,
    error_buffer: CodexIosErrorBuffer,
) -> CodexIosStatus {
    let result = (|| {
        let bridge = bridge_for_handle(session_handle)?;
        let call_id = read_ffi_string(call_id)?;
        let cell_id = CellId::new(read_ffi_string(cell_id)?);
        let text = read_ffi_string(text)?;
        let cell = bridge.cell(&cell_id)?;
        bridge
            .runtime
            .block_on(cell.delegate.notify(call_id, cell_id, text, cell.token))
    })();

    match result {
        Ok(()) => CodexIosStatus::Accepted,
        Err(error) => ffi_error(error_buffer, error),
    }
}

#[cfg(test)]
#[path = "ios_session_tests.rs"]
mod tests;

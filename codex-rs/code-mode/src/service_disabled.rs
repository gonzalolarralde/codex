use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
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
use codex_code_mode_protocol::NotificationFuture;
use codex_code_mode_protocol::RuntimeResponse;
use codex_code_mode_protocol::StartedCell;
use codex_code_mode_protocol::ToolInvocationFuture;
use codex_code_mode_protocol::WaitOutcome;
use codex_code_mode_protocol::WaitRequest;
use codex_ios_platform::CodexIosErrorBuffer;
use codex_ios_platform::CodexIosHandle;
use codex_ios_platform::CodexIosStatus;
use codex_ios_platform::CodexIosString;
use serde_json::json;
use tokio::runtime::Handle;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

static NEXT_SESSION_HANDLE: AtomicU64 = AtomicU64::new(1);
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
    delegate: Arc<dyn CodeModeSessionDelegate>,
    runtime: Handle,
    cell_tokens: Mutex<HashMap<CellId, CancellationToken>>,
}

impl SessionBridge {
    fn token(&self, cell_id: &CellId) -> CancellationToken {
        self.cell_tokens
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(cell_id.clone())
            .or_default()
            .clone()
    }

    fn close_cell(&self, cell_id: &CellId) {
        if let Some(token) = self
            .cell_tokens
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(cell_id)
        {
            token.cancel();
        }
        self.delegate.cell_closed(cell_id);
    }

    fn shutdown(&self) {
        let cell_ids = self
            .cell_tokens
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

pub struct NoopCodeModeSessionDelegate;

impl CodeModeSessionDelegate for NoopCodeModeSessionDelegate {
    fn invoke_tool<'a>(
        &'a self,
        invocation: CodeModeNestedToolCall,
        _cancellation_token: CancellationToken,
    ) -> ToolInvocationFuture<'a> {
        Box::pin(async move {
            Err(disabled_message(
                "delegate.invokeTool",
                json!({
                    "cellId": invocation.cell_id,
                    "runtimeToolCallId": invocation.runtime_tool_call_id,
                    "toolName": invocation.tool_name,
                    "toolKind": invocation.tool_kind,
                    "input": invocation.input,
                }),
            ))
        })
    }

    fn notify<'a>(
        &'a self,
        _call_id: String,
        _cell_id: CellId,
        _text: String,
        _cancellation_token: CancellationToken,
    ) -> NotificationFuture<'a> {
        Box::pin(async { Ok(()) })
    }

    fn cell_closed(&self, _cell_id: &CellId) {}
}

#[derive(Default)]
pub struct InProcessCodeModeSessionProvider;

impl CodeModeSessionProvider for InProcessCodeModeSessionProvider {
    fn create_session<'a>(
        &'a self,
        delegate: Arc<dyn CodeModeSessionDelegate>,
    ) -> CodeModeSessionProviderFuture<'a> {
        Box::pin(async move {
            Ok(Arc::new(CodeModeService::with_delegate(delegate)) as Arc<dyn CodeModeSession>)
        })
    }
}

pub struct CodeModeService {
    session_handle: CodexIosHandle,
    bridge: Arc<SessionBridge>,
    initialized: Mutex<bool>,
    alive: AtomicBool,
}

impl Default for CodeModeService {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeModeService {
    pub fn new() -> Self {
        Self::with_delegate(Arc::new(NoopCodeModeSessionDelegate))
    }

    pub fn with_delegate(delegate: Arc<dyn CodeModeSessionDelegate>) -> Self {
        let session_handle = NEXT_SESSION_HANDLE.fetch_add(1, Ordering::Relaxed);
        let bridge = Arc::new(SessionBridge {
            delegate,
            runtime: Handle::current(),
            cell_tokens: Mutex::new(HashMap::new()),
        });
        session_bridges()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(session_handle, bridge.clone());
        Self {
            session_handle,
            bridge,
            initialized: Mutex::new(false),
            alive: AtomicBool::new(true),
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

    pub async fn execute(&self, request: ExecuteRequest) -> Result<StartedCell, String> {
        self.ensure_initialized()?;
        let request_json = serde_json::to_string(&request).map_err(|err| err.to_string())?;
        match codex_ios_platform::code_mode_execute(self.session_handle, &request_json) {
            Ok(response_json) => {
                let response: RuntimeResponse =
                    serde_json::from_str(&response_json).map_err(|err| err.to_string())?;
                self.finish_response(&response);
                let cell_id = response_cell_id(&response);
                let (response_tx, response_rx) = oneshot::channel();
                let _ = response_tx.send(Ok(response));
                Ok(StartedCell::from_result_receiver(cell_id, response_rx))
            }
            Err(codex_ios_platform::IosPlatformError::Error(message)) => Err(message),
            Err(codex_ios_platform::IosPlatformError::Unsupported(_)) => Err(disabled_message(
                "execute",
                json!({
                    "toolCallId": request.tool_call_id,
                    "source": request.source,
                    "yieldTimeMs": request.yield_time_ms,
                    "maxOutputTokens": request.max_output_tokens,
                    "enabledToolCount": request.enabled_tools.len(),
                }),
            )),
        }
    }

    pub async fn wait(&self, request: WaitRequest) -> Result<WaitOutcome, String> {
        match codex_ios_platform::code_mode_wait(
            self.session_handle,
            request.cell_id.as_str(),
            request.yield_time_ms,
        ) {
            Ok(response_json) => {
                let outcome: WaitOutcome =
                    serde_json::from_str(&response_json).map_err(|err| err.to_string())?;
                let response: RuntimeResponse = match &outcome {
                    WaitOutcome::LiveCell(response) | WaitOutcome::MissingCell(response) => {
                        response.clone()
                    }
                };
                self.finish_response(&response);
                Ok(outcome)
            }
            Err(codex_ios_platform::IosPlatformError::Error(message)) => Err(message),
            Err(codex_ios_platform::IosPlatformError::Unsupported(_)) => Err(disabled_message(
                "wait",
                json!({
                    "cellId": request.cell_id,
                    "yieldTimeMs": request.yield_time_ms,
                }),
            )),
        }
    }

    pub async fn terminate(&self, cell_id: CellId) -> Result<WaitOutcome, String> {
        self.bridge.token(&cell_id).cancel();
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
        self.alive.store(false, Ordering::Release);
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

impl Drop for CodeModeService {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Release);
        self.bridge.shutdown();
        session_bridges()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.session_handle);
    }
}

impl CodeModeSession for CodeModeService {
    fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }

    fn execute<'a>(
        &'a self,
        request: ExecuteRequest,
    ) -> CodeModeSessionResultFuture<'a, StartedCell> {
        Box::pin(CodeModeService::execute(self, request))
    }

    fn wait<'a>(&'a self, request: WaitRequest) -> CodeModeSessionResultFuture<'a, WaitOutcome> {
        Box::pin(CodeModeService::wait(self, request))
    }

    fn terminate<'a>(&'a self, cell_id: CellId) -> CodeModeSessionResultFuture<'a, WaitOutcome> {
        Box::pin(CodeModeService::terminate(self, cell_id))
    }

    fn shutdown<'a>(&'a self) -> CodeModeSessionResultFuture<'a, ()> {
        Box::pin(CodeModeService::shutdown(self))
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
        let cancellation_token = bridge.token(&invocation.cell_id);
        let result = bridge
            .runtime
            .block_on(bridge.delegate.invoke_tool(invocation, cancellation_token))?;
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
        let cancellation_token = bridge.token(&cell_id);
        bridge.runtime.block_on(
            bridge
                .delegate
                .notify(call_id, cell_id, text, cancellation_token),
        )
    })();

    match result {
        Ok(()) => CodexIosStatus::Accepted,
        Err(error) => ffi_error(error_buffer, error),
    }
}

use std::sync::Arc;

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
use serde_json::json;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

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
        _delegate: Arc<dyn CodeModeSessionDelegate>,
    ) -> CodeModeSessionProviderFuture<'a> {
        Box::pin(async { Ok(Arc::new(CodeModeService) as Arc<dyn CodeModeSession>) })
    }
}

#[derive(Default)]
pub struct CodeModeService;

impl CodeModeService {
    pub fn new() -> Self {
        Self
    }

    pub fn with_delegate(_delegate: Arc<dyn CodeModeSessionDelegate>) -> Self {
        Self
    }

    pub async fn execute(&self, request: ExecuteRequest) -> Result<StartedCell, String> {
        let request_json = serde_json::to_string(&request).map_err(|err| err.to_string())?;
        match codex_ios_platform::code_mode_execute(&request_json) {
            Ok(response_json) => {
                let response: RuntimeResponse =
                    serde_json::from_str(&response_json).map_err(|err| err.to_string())?;
                let cell_id = response_cell_id(&response);
                let (response_tx, response_rx) = oneshot::channel();
                let _ = response_tx.send(Ok(response));
                return Ok(StartedCell::from_result_receiver(cell_id, response_rx));
            }
            Err(codex_ios_platform::IosPlatformError::Error(message)) => return Err(message),
            Err(codex_ios_platform::IosPlatformError::Unsupported(_)) => {}
        }

        Err(disabled_message(
            "execute",
            json!({
                "toolCallId": request.tool_call_id,
                "source": request.source,
                "yieldTimeMs": request.yield_time_ms,
                "maxOutputTokens": request.max_output_tokens,
                "enabledToolCount": request.enabled_tools.len(),
            }),
        ))
    }

    pub async fn wait(&self, request: WaitRequest) -> Result<WaitOutcome, String> {
        match codex_ios_platform::code_mode_wait(request.cell_id.as_str(), request.yield_time_ms) {
            Ok(response_json) => {
                return serde_json::from_str(&response_json).map_err(|err| err.to_string());
            }
            Err(codex_ios_platform::IosPlatformError::Error(message)) => return Err(message),
            Err(codex_ios_platform::IosPlatformError::Unsupported(_)) => {}
        }

        Err(disabled_message(
            "wait",
            json!({
                "cellId": request.cell_id,
                "yieldTimeMs": request.yield_time_ms,
            }),
        ))
    }

    pub async fn terminate(&self, cell_id: CellId) -> Result<WaitOutcome, String> {
        match codex_ios_platform::code_mode_terminate(cell_id.as_str()) {
            Ok(response_json) => {
                return serde_json::from_str(&response_json).map_err(|err| err.to_string());
            }
            Err(codex_ios_platform::IosPlatformError::Error(message)) => return Err(message),
            Err(codex_ios_platform::IosPlatformError::Unsupported(_)) => {}
        }

        Err(disabled_message(
            "terminate",
            json!({
                "cellId": cell_id,
            }),
        ))
    }

    pub async fn shutdown(&self) -> Result<(), String> {
        Ok(())
    }
}

impl CodeModeSession for CodeModeService {
    fn is_alive(&self) -> bool {
        false
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

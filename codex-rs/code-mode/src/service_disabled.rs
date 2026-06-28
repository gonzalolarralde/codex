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
use codex_code_mode_protocol::StartedCell;
use codex_code_mode_protocol::ToolInvocationFuture;
use codex_code_mode_protocol::WaitOutcome;
use codex_code_mode_protocol::WaitRequest;
use serde_json::json;
use tokio_util::sync::CancellationToken;

fn disabled_message(action: &str, payload: serde_json::Value) -> String {
    let payload_json = serde_json::to_string(&json!({
        "action": action,
        "payload": payload,
    }))
    .unwrap_or_else(|_| "{}".to_string());
    codex_ios_platform::unsupported_message(
        codex_ios_platform::OPERATION_JAVASCRIPT_RUNTIME,
        &payload_json,
    )
    .unwrap_or_else(|| {
        codex_ios_platform::generic_unsupported_message(
            codex_ios_platform::OPERATION_JAVASCRIPT_RUNTIME,
        )
    })
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
        Err(disabled_message(
            "wait",
            json!({
                "cellId": request.cell_id,
                "yieldTimeMs": request.yield_time_ms,
            }),
        ))
    }

    pub async fn terminate(&self, cell_id: CellId) -> Result<WaitOutcome, String> {
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

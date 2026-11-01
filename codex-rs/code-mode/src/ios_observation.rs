use codex_code_mode_protocol::FunctionCallOutputContentItem;
use codex_code_mode_protocol::RuntimeResponse;
use codex_code_mode_protocol::WaitOutcome;
use codex_code_mode_protocol::WaitRequest;
use codex_ios_platform::CodexIosHandle;
use tokio_util::sync::CancellationToken;

const PREEMPT_POLL_INTERVAL_MS: u128 = 50;

pub(super) async fn wait(
    session_handle: CodexIosHandle,
    request: WaitRequest,
    preempt: Option<CancellationToken>,
) -> Result<WaitOutcome, String> {
    tokio::task::spawn_blocking(move || {
        let deadline = std::time::Instant::now()
            .checked_add(std::time::Duration::from_millis(request.yield_time_ms))
            .ok_or_else(|| "iOS code-mode wait duration is too large".to_string())?;
        let mut observed: Vec<FunctionCallOutputContentItem> = Vec::new();
        loop {
            let interrupted = preempt
                .as_ref()
                .is_some_and(CancellationToken::is_cancelled);
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            let interval = if interrupted {
                0
            } else if preempt.is_some() {
                remaining.as_millis().min(PREEMPT_POLL_INTERVAL_MS) as u64
            } else {
                remaining.as_millis() as u64
            };
            let json = codex_ios_platform::code_mode_wait(
                session_handle,
                request.cell_id.as_str(),
                interval,
            )
            .map_err(|error| error.to_string())?;
            let mut outcome: WaitOutcome =
                serde_json::from_str(&json).map_err(|error| error.to_string())?;
            let response = match &mut outcome {
                WaitOutcome::LiveCell(response) | WaitOutcome::MissingCell(response) => response,
            };
            let yielded = matches!(response, RuntimeResponse::Yielded { .. });
            let content_items = match response {
                RuntimeResponse::Yielded { content_items, .. }
                | RuntimeResponse::Terminated { content_items, .. }
                | RuntimeResponse::Result { content_items, .. } => content_items,
            };
            observed.append(content_items);
            if !yielded
                || !observed.is_empty()
                || preempt.is_none()
                || interrupted
                || preempt
                    .as_ref()
                    .is_some_and(CancellationToken::is_cancelled)
                || std::time::Instant::now() >= deadline
            {
                *content_items = observed;
                return Ok(outcome);
            }
        }
    })
    .await
    .map_err(|error| error.to_string())?
}

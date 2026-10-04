use super::*;
use codex_code_mode_protocol::NotificationFuture;
use codex_code_mode_protocol::ToolInvocationFuture;
use codex_ios_platform::CODEX_IOS_PLATFORM_CALLBACKS_VERSION;
use codex_ios_platform::CodexIosPlatformCallbacks;
use pretty_assertions::assert_eq;
use std::ffi::CStr;
use std::sync::atomic::AtomicBool;

static CREATED_HANDLE: AtomicU64 = AtomicU64::new(/*v*/ 0);
static SHUT_DOWN: AtomicBool = AtomicBool::new(/*v*/ false);

#[derive(Default)]
struct Delegate {
    calls: Mutex<Vec<serde_json::Value>>,
    token: Mutex<Option<CancellationToken>>,
}

impl CodeModeSessionDelegate for Delegate {
    fn invoke_tool<'a>(
        &'a self,
        invocation: CodeModeNestedToolCall,
        cancellation_token: CancellationToken,
    ) -> ToolInvocationFuture<'a> {
        Box::pin(async move {
            let invocation = serde_json::to_value(invocation).unwrap();
            self.calls.lock().unwrap().push(invocation.clone());
            *self.token.lock().unwrap() = Some(cancellation_token);
            Ok(invocation)
        })
    }

    fn notify<'a>(
        &'a self,
        call_id: String,
        cell_id: CellId,
        text: String,
        _cancellation_token: CancellationToken,
    ) -> NotificationFuture<'a> {
        Box::pin(async move {
            self.calls
                .lock()
                .unwrap()
                .push(json!([call_id, cell_id, text]));
            Ok(())
        })
    }

    fn cell_closed(&self, cell_id: &CellId) {
        self.calls
            .lock()
            .unwrap()
            .push(json!({ "closed": cell_id }));
    }
}

struct CallbacksGuard;

impl Drop for CallbacksGuard {
    fn drop(&mut self) {
        codex_ios_platform::set_callbacks(/*callbacks_to_set*/ None);
    }
}

fn string(value: &str) -> CodexIosString {
    CodexIosString {
        ptr: value.as_ptr().cast(),
        len: value.len(),
    }
}

fn output(bytes: &mut [u8]) -> CodexIosErrorBuffer {
    CodexIosErrorBuffer {
        message_buffer: bytes.as_mut_ptr().cast(),
        message_buffer_len: bytes.len(),
    }
}

unsafe extern "C" fn create(
    _context: *mut std::ffi::c_void,
    handle: u64,
    _buffer: CodexIosErrorBuffer,
) -> CodexIosStatus {
    CREATED_HANDLE.store(handle, Ordering::SeqCst);
    CodexIosStatus::Accepted
}

unsafe extern "C" fn execute(
    _context: *mut std::ffi::c_void,
    _handle: u64,
    request: CodexIosString,
    buffer: CodexIosErrorBuffer,
) -> CodexIosStatus {
    let request: ExecuteRequest = serde_json::from_str(&read_ffi_string(request).unwrap()).unwrap();
    let response = RuntimeResponse::Yielded {
        cell_id: CellId::new(request.tool_call_id),
        content_items: vec![],
        code_mode_host_duration: None,
    };
    write_ffi_buffer(buffer, &serde_json::to_string(&response).unwrap());
    CodexIosStatus::Accepted
}

unsafe extern "C" fn terminate(
    _context: *mut std::ffi::c_void,
    _handle: u64,
    cell: CodexIosString,
    buffer: CodexIosErrorBuffer,
) -> CodexIosStatus {
    let response = WaitOutcome::LiveCell(RuntimeResponse::Terminated {
        cell_id: CellId::new(read_ffi_string(cell).unwrap()),
        content_items: vec![],
        code_mode_host_duration: None,
    });
    write_ffi_buffer(buffer, &serde_json::to_string(&response).unwrap());
    CodexIosStatus::Accepted
}

unsafe extern "C" fn wait(
    _context: *mut std::ffi::c_void,
    _handle: u64,
    cell: CodexIosString,
    yield_time_ms: u64,
    buffer: CodexIosErrorBuffer,
) -> CodexIosStatus {
    assert!(yield_time_ms <= 50);
    std::thread::sleep(std::time::Duration::from_millis(yield_time_ms));
    let response = WaitOutcome::LiveCell(RuntimeResponse::Yielded {
        cell_id: CellId::new(read_ffi_string(cell).unwrap()),
        content_items: vec![],
        code_mode_host_duration: None,
    });
    write_ffi_buffer(buffer, &serde_json::to_string(&response).unwrap());
    CodexIosStatus::Accepted
}

unsafe extern "C" fn shutdown(
    _context: *mut std::ffi::c_void,
    _handle: u64,
    _buffer: CodexIosErrorBuffer,
) -> CodexIosStatus {
    SHUT_DOWN.store(/*val*/ true, Ordering::SeqCst);
    CodexIosStatus::Accepted
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::serial]
async fn callbacks_preserve_nested_tools_notifications_and_cancellation() {
    let _guard = CallbacksGuard;
    SHUT_DOWN.store(/*val*/ false, Ordering::SeqCst);
    codex_ios_platform::set_callbacks(Some(CodexIosPlatformCallbacks {
        version: CODEX_IOS_PLATFORM_CALLBACKS_VERSION,
        struct_size: std::mem::size_of::<CodexIosPlatformCallbacks>(),
        context: std::ptr::null_mut(),
        unsupported_operation: None,
        log_message: None,
        process_spawn: None,
        process_write_stdin: None,
        process_resize_pty: None,
        process_terminate: None,
        code_mode_create_session: Some(create),
        code_mode_execute: Some(execute),
        code_mode_wait: Some(wait),
        code_mode_terminate: Some(terminate),
        code_mode_shutdown: Some(shutdown),
        git_run: None,
        git_apply_patch: None,
        git_stage_paths: None,
        shell_snapshot: None,
        mcp_stdio_launch: None,
        doctor_report: None,
    }));
    let delegate = Arc::new(Delegate::default());
    let session = IosCodeModeSessionProvider.create_session().await.unwrap();
    let started = session
        .execute(
            ExecuteRequest {
                tool_call_id: "cell-1".into(),
                enabled_tools: vec![],
                source: "text(await tools.lookup({}))".into(),
                yield_time_ms: None,
                max_output_tokens: None,
            },
            delegate.clone(),
            /*preempt*/ None,
        )
        .await
        .unwrap();
    let cell_id = CellId::new("cell-1".into());
    assert_eq!(started.cell_id, cell_id);
    assert_eq!(
        started.initial_response().await.unwrap(),
        RuntimeResponse::Yielded {
            cell_id: cell_id.clone(),
            content_items: vec![],
            code_mode_host_duration: None,
        }
    );
    let duplicate = session
        .execute(
            ExecuteRequest {
                tool_call_id: "cell-1".into(),
                enabled_tools: vec![],
                source: "text('duplicate')".into(),
                yield_time_ms: None,
                max_output_tokens: None,
            },
            Arc::new(Delegate::default()),
            /*preempt*/ None,
        )
        .await;
    assert_eq!(
        duplicate.err(),
        Some("iOS code-mode cell cell-1 is already running".to_string())
    );
    let handle = CREATED_HANDLE.load(Ordering::SeqCst);
    let invocation = json!({
        "cell_id": "cell-1", "runtime_tool_call_id": "nested-1",
        "tool_name": { "name": "lookup", "namespace": null },
        "tool_kind": "function", "input": { "key": "value" }
    });
    let request = invocation.to_string();
    tokio::task::spawn_blocking(move || {
        let mut bytes = vec![0_u8; 4096];
        assert_eq!(
            codex_ios_code_mode_invoke_tool(handle, string(&request), output(&mut bytes)),
            CodexIosStatus::Accepted
        );
        let text = CStr::from_bytes_until_nul(&bytes)
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(text).unwrap(),
            serde_json::from_str::<serde_json::Value>(&request).unwrap()
        );
        assert_eq!(
            codex_ios_code_mode_notify(
                handle,
                string("root"),
                string("cell-1"),
                string("progress"),
                output(&mut bytes)
            ),
            CodexIosStatus::Accepted
        );
    })
    .await
    .unwrap();
    let preempt = CancellationToken::new();
    let signal = preempt.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(/*millis*/ 10)).await;
        signal.cancel();
    });
    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(/*secs*/ 2),
        session.wait(
            WaitRequest {
                cell_id: cell_id.clone(),
                yield_time_ms: 10_000,
            },
            Some(preempt),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        outcome,
        WaitOutcome::LiveCell(RuntimeResponse::Yielded {
            cell_id: cell_id.clone(),
            content_items: vec![],
            code_mode_host_duration: None,
        })
    );
    assert!(
        !delegate
            .token
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .is_cancelled()
    );
    assert_eq!(
        session.terminate(cell_id.clone()).await.unwrap(),
        WaitOutcome::LiveCell(RuntimeResponse::Terminated {
            cell_id: cell_id.clone(),
            content_items: vec![],
            code_mode_host_duration: None,
        })
    );
    assert!(
        delegate
            .token
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .is_cancelled()
    );
    assert_eq!(
        *delegate.calls.lock().unwrap(),
        vec![
            invocation,
            json!(["root", "cell-1", "progress"]),
            json!({ "closed": "cell-1" })
        ]
    );
    let first_calls = delegate.calls.lock().unwrap().clone();
    let second_delegate = Arc::new(Delegate::default());
    let second = session
        .execute(
            ExecuteRequest {
                tool_call_id: "cell-2".into(),
                enabled_tools: vec![],
                source: "text(await tools.lookup({}))".into(),
                yield_time_ms: None,
                max_output_tokens: None,
            },
            second_delegate.clone(),
            /*preempt*/ None,
        )
        .await
        .unwrap();
    let second_response = second.initial_response().await.unwrap();
    assert_eq!(
        second_response,
        RuntimeResponse::Yielded {
            cell_id: CellId::new("cell-2".into()),
            content_items: vec![],
            code_mode_host_duration: None,
        }
    );
    tokio::task::spawn_blocking(move || {
        let mut bytes = vec![0_u8; 4096];
        assert_eq!(
            codex_ios_code_mode_notify(
                handle,
                string("root-2"),
                string("cell-2"),
                string("next"),
                output(&mut bytes)
            ),
            CodexIosStatus::Accepted
        );
    })
    .await
    .unwrap();
    session.shutdown().await.unwrap();
    assert_eq!(
        *second_delegate.calls.lock().unwrap(),
        vec![
            json!(["root-2", "cell-2", "next"]),
            json!({ "closed": "cell-2" }),
        ]
    );
    assert_eq!(*delegate.calls.lock().unwrap(), first_calls);
    assert!(SHUT_DOWN.load(Ordering::SeqCst));
    assert!(bridge_for_handle(handle).is_err());
}

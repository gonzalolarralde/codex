use std::collections::HashMap;
use std::os::fd::FromRawFd;
use std::os::fd::OwnedFd;
use std::os::raw::c_int;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;

use codex_arg0::Arg0DispatchPaths;
use codex_config::LoaderOverrides;
use codex_protocol::protocol::SessionSource;
use codex_utils_cli::CliConfigOverrides;
use tokio_util::sync::CancellationToken;

use crate::AppServerRuntimeOptions;
use crate::AppServerWebsocketAuthSettings;
use crate::PluginStartupTasks;
use crate::RemoteControlStartupMode;
use crate::run_main_with_stdio_io;

static NEXT_HANDLE_ID: AtomicU64 = AtomicU64::new(1);
static HANDLES: OnceLock<Mutex<HashMap<u64, IosAppServerHandle>>> = OnceLock::new();
const TOKIO_WORKER_STACK_SIZE_BYTES: usize = 16 * 1024 * 1024;

struct IosAppServerHandle {
    shutdown_token: CancellationToken,
    thread: JoinHandle<std::io::Result<()>>,
}

fn handles() -> &'static Mutex<HashMap<u64, IosAppServerHandle>> {
    HANDLES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Starts codex-app-server on iOS using two Foundation Pipe file descriptors.
///
/// The caller transfers ownership of both file descriptors to Rust. `input_read_fd`
/// is the read end of the pipe Swift writes JSON-RPC messages into, and
/// `output_write_fd` is the write end of the pipe Swift reads JSON-RPC messages from.
/// Returns 0 if the runtime thread could not be started.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn codex_app_server_ios_start_with_stdio_fds(
    input_read_fd: c_int,
    output_write_fd: c_int,
) -> u64 {
    if input_read_fd < 0 || output_write_fd < 0 {
        return 0;
    }

    let handle_id = NEXT_HANDLE_ID.fetch_add(1, Ordering::Relaxed);
    if handle_id == 0 {
        return 0;
    }

    let input = unsafe { OwnedFd::from_raw_fd(input_read_fd) };
    let output = unsafe { OwnedFd::from_raw_fd(output_write_fd) };
    let shutdown_token = CancellationToken::new();
    let thread_shutdown_token = shutdown_token.clone();
    let thread = match std::thread::Builder::new()
        .name("codex-app-server-ios".to_string())
        .stack_size(TOKIO_WORKER_STACK_SIZE_BYTES)
        .spawn(move || {
            let input = std::fs::File::from(input);
            let output = std::fs::File::from(output);
            let input = tokio::fs::File::from_std(input);
            let output = tokio::fs::File::from_std(output);
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_stack_size(TOKIO_WORKER_STACK_SIZE_BYTES)
                .build()?;
            runtime.block_on(run_main_with_stdio_io(
                Arg0DispatchPaths::default(),
                CliConfigOverrides::default(),
                LoaderOverrides::without_managed_config_for_tests(),
                /*strict_config*/ false,
                /*default_analytics_enabled*/ false,
                input,
                output,
                SessionSource::VSCode,
                AppServerWebsocketAuthSettings::default(),
                AppServerRuntimeOptions {
                    plugin_startup_tasks: PluginStartupTasks::Skip,
                    remote_control_startup_mode: RemoteControlStartupMode::DisabledEphemeral,
                    install_shutdown_signal_handler: false,
                    shutdown_token: Some(thread_shutdown_token),
                },
            ))
        }) {
        Ok(thread) => thread,
        Err(_) => return 0,
    };

    let mut handles = handles()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    handles.insert(
        handle_id,
        IosAppServerHandle {
            shutdown_token,
            thread,
        },
    );
    handle_id
}

/// Requests shutdown for an iOS app-server started with
/// [`codex_app_server_ios_start_with_stdio_fds`] without waiting for its runtime
/// thread to exit.
///
/// Returns 0 when the stop request was delivered, and -1 for an unknown handle.
#[unsafe(no_mangle)]
pub extern "C" fn codex_app_server_ios_request_stop(handle_id: u64) -> c_int {
    let handles = handles()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(handle) = handles.get(&handle_id) else {
        return -1;
    };

    handle.shutdown_token.cancel();
    0
}

/// Requests shutdown for an iOS app-server started with
/// [`codex_app_server_ios_start_with_stdio_fds`] and waits for its runtime thread.
///
/// Returns 0 on clean shutdown, -1 for an unknown handle, -2 if app-server
/// returned an error, and -3 if the runtime thread panicked.
#[unsafe(no_mangle)]
pub extern "C" fn codex_app_server_ios_stop(handle_id: u64) -> c_int {
    let handle = {
        let mut handles = handles()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        handles.remove(&handle_id)
    };
    let Some(handle) = handle else {
        return -1;
    };

    handle.shutdown_token.cancel();
    match handle.thread.join() {
        Ok(Ok(())) => 0,
        Ok(Err(_)) => -2,
        Err(_) => -3,
    }
}

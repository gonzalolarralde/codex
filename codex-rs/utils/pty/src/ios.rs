use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use tokio::sync::mpsc;

use crate::ProcessDriver;
use crate::SpawnedProcess;
use crate::TerminalSize;
use crate::spawn_from_driver;

pub async fn spawn_process(
    purpose: codex_ios_platform::ProcessPurpose,
    program: &str,
    args: &[String],
    cwd: &Path,
    env: &HashMap<String, String>,
    arg0: &Option<String>,
    tty: bool,
    stream_stdin: bool,
    size: TerminalSize,
) -> Result<SpawnedProcess> {
    let process = codex_ios_platform::process_spawn(codex_ios_platform::ProcessSpawnConfig {
        purpose,
        program,
        args,
        cwd: &cwd.display().to_string(),
        env,
        arg0: arg0.as_deref(),
        tty,
        stream_stdin,
        size: codex_ios_platform::TerminalSize {
            cols: size.cols,
            rows: size.rows,
        },
    })?;
    let handle = process.handle;
    let (writer_tx, mut writer_rx) = mpsc::channel::<Vec<u8>>(256);
    let writer_handle = tokio::spawn(async move {
        while let Some(bytes) = writer_rx.recv().await {
            let _ = codex_ios_platform::process_write_stdin(handle, &bytes, false);
        }
        let _ = codex_ios_platform::process_write_stdin(handle, &[], true);
    });
    let terminator_handle = process.handle;
    let resize_handle = process.handle;

    Ok(spawn_from_driver(ProcessDriver {
        writer_tx,
        stdout_rx: process.stdout_rx,
        stderr_rx: Some(process.stderr_rx),
        exit_rx: process.exit_rx,
        terminator: Some(Box::new(move || {
            let _ = codex_ios_platform::process_terminate(terminator_handle);
        })),
        writer_handle: Some(writer_handle),
        resizer: Some(Box::new(move |size| {
            codex_ios_platform::process_resize_pty(
                resize_handle,
                codex_ios_platform::TerminalSize {
                    cols: size.cols,
                    rows: size.rows,
                },
            )
            .map_err(Into::into)
        })),
    }))
}

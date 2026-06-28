use super::CHANNEL_CAPACITY;
use super::ConnectionOrigin;
use super::TransportEvent;
use super::forward_incoming_message;
use super::next_connection_id;
use super::serialize_outgoing_message;
use crate::outgoing_message::QueuedOutgoingMessage;
use codex_app_server_protocol::InitializeParams;
use codex_app_server_protocol::JSONRPCMessage;
use codex_app_server_protocol::JSONRPCRequest;
use std::io::ErrorKind;
use std::io::Result as IoResult;
use tokio::io;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::debug;
use tracing::error;
use tracing::info;

pub async fn start_stdio_connection(
    transport_event_tx: mpsc::Sender<TransportEvent>,
    stdio_handles: &mut Vec<JoinHandle<()>>,
    initialize_client_name_tx: oneshot::Sender<String>,
) -> IoResult<()> {
    start_stdio_connection_with_io(
        transport_event_tx,
        stdio_handles,
        initialize_client_name_tx,
        io::stdin(),
        io::stdout(),
    )
    .await
}

pub async fn start_stdio_connection_with_io<R, W>(
    transport_event_tx: mpsc::Sender<TransportEvent>,
    stdio_handles: &mut Vec<JoinHandle<()>>,
    initialize_client_name_tx: oneshot::Sender<String>,
    reader: R,
    writer: W,
) -> IoResult<()>
where
    R: AsyncRead + Send + Unpin + 'static,
    W: AsyncWrite + Send + Unpin + 'static,
{
    start_stdio_connection_with_io_and_shutdown(
        transport_event_tx,
        stdio_handles,
        initialize_client_name_tx,
        reader,
        writer,
        None,
    )
    .await
}

pub async fn start_stdio_connection_with_io_and_shutdown<R, W>(
    transport_event_tx: mpsc::Sender<TransportEvent>,
    stdio_handles: &mut Vec<JoinHandle<()>>,
    initialize_client_name_tx: oneshot::Sender<String>,
    reader: R,
    writer: W,
    shutdown_token: Option<CancellationToken>,
) -> IoResult<()>
where
    R: AsyncRead + Send + Unpin + 'static,
    W: AsyncWrite + Send + Unpin + 'static,
{
    let connection_id = next_connection_id();
    let (writer_tx, mut writer_rx) = mpsc::channel::<QueuedOutgoingMessage>(CHANNEL_CAPACITY);
    let writer_tx_for_reader = writer_tx.clone();
    transport_event_tx
        .send(TransportEvent::ConnectionOpened {
            connection_id,
            origin: ConnectionOrigin::Stdio,
            writer: writer_tx,
            disconnect_sender: None,
        })
        .await
        .map_err(|_| std::io::Error::new(ErrorKind::BrokenPipe, "processor unavailable"))?;

    let transport_event_tx_for_reader = transport_event_tx.clone();
    stdio_handles.push(tokio::spawn(async move {
        let reader = BufReader::new(reader);
        let mut lines = reader.lines();
        let mut initialize_client_name_tx = Some(initialize_client_name_tx);

        loop {
            let next_line = match &shutdown_token {
                Some(shutdown_token) => {
                    tokio::select! {
                        _ = shutdown_token.cancelled() => break,
                        next_line = lines.next_line() => next_line,
                    }
                }
                None => lines.next_line().await,
            };

            match next_line {
                Ok(Some(line)) => {
                    if let Some(client_name) = stdio_initialize_client_name(&line)
                        && let Some(initialize_client_name_tx) = initialize_client_name_tx.take()
                    {
                        let _ = initialize_client_name_tx.send(client_name);
                    }
                    if !forward_incoming_message(
                        &transport_event_tx_for_reader,
                        &writer_tx_for_reader,
                        connection_id,
                        &line,
                    )
                    .await
                    {
                        break;
                    }
                }
                Ok(None) => break,
                Err(err) => {
                    error!("Failed reading stdin: {err}");
                    break;
                }
            }
        }

        let _ = transport_event_tx_for_reader
            .send(TransportEvent::ConnectionClosed { connection_id })
            .await;
        debug!("stdin reader finished (EOF)");
    }));

    stdio_handles.push(tokio::spawn(async move {
        let mut writer = writer;
        while let Some(queued_message) = writer_rx.recv().await {
            let Some(mut json) = serialize_outgoing_message(queued_message.message) else {
                continue;
            };
            json.push('\n');
            if let Err(err) = writer.write_all(json.as_bytes()).await {
                error!("Failed to write to stdout: {err}");
                break;
            }
            if let Err(err) = writer.flush().await {
                error!("Failed to flush stdout: {err}");
                break;
            }
            if let Some(write_complete_tx) = queued_message.write_complete_tx {
                let _ = write_complete_tx.send(());
            }
        }
        info!("stdout writer exited (channel closed)");
    }));

    Ok(())
}

fn stdio_initialize_client_name(line: &str) -> Option<String> {
    let message = serde_json::from_str::<JSONRPCMessage>(line).ok()?;
    let JSONRPCMessage::Request(JSONRPCRequest { method, params, .. }) = message else {
        return None;
    };
    if method != "initialize" {
        return None;
    }
    let params = serde_json::from_value::<InitializeParams>(params?).ok()?;
    Some(params.client_info.name)
}

#[cfg(test)]
#[path = "stdio_tests.rs"]
mod tests;

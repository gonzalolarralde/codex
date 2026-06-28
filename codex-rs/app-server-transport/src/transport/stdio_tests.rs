use super::*;
use pretty_assertions::assert_eq;
use tokio::time::Duration;
use tokio::time::timeout;

#[tokio::test]
async fn custom_stdio_reader_exits_when_shutdown_is_requested() {
    let (transport_event_tx, mut transport_event_rx) = mpsc::channel(4);
    let mut handles = Vec::new();
    let (initialize_client_name_tx, _initialize_client_name_rx) = oneshot::channel();
    let (_client_writer, server_reader) = tokio::io::duplex(64);
    let shutdown_token = CancellationToken::new();

    start_stdio_connection_with_io_and_shutdown(
        transport_event_tx,
        &mut handles,
        initialize_client_name_tx,
        server_reader,
        tokio::io::sink(),
        Some(shutdown_token.clone()),
    )
    .await
    .expect("stdio connection should start");

    let opened_connection_id = match transport_event_rx
        .recv()
        .await
        .expect("connection should open")
    {
        TransportEvent::ConnectionOpened {
            connection_id,
            origin,
            writer,
            disconnect_sender,
        } => {
            assert_eq!(ConnectionOrigin::Stdio, origin);
            assert!(disconnect_sender.is_none());
            drop(writer);
            connection_id
        }
        event => panic!("expected connection opened event, got {event:?}"),
    };

    shutdown_token.cancel();

    let closed_connection_id = match timeout(Duration::from_secs(1), transport_event_rx.recv())
        .await
        .expect("shutdown should close the connection")
        .expect("connection close event should be sent")
    {
        TransportEvent::ConnectionClosed { connection_id } => connection_id,
        event => panic!("expected connection closed event, got {event:?}"),
    };
    assert_eq!(opened_connection_id, closed_connection_id);

    for handle in handles {
        timeout(Duration::from_secs(1), handle)
            .await
            .expect("stdio task should finish")
            .expect("stdio task should not panic");
    }
}

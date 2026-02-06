use krill_common::{ClientMessage, ServiceStatus};
use krill_sdk_rust::{KrillClient, KrillError};
use std::collections::HashMap;
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::UnixListener;

/// Read a single newline-terminated JSON line from a UnixListener and parse it.
async fn accept_and_read_message(listener: &UnixListener) -> ClientMessage {
    let (stream, _) = listener
        .accept()
        .await
        .expect("failed to accept connection");
    let reader = BufReader::new(stream);
    let mut lines = reader.lines();
    let line = lines
        .next_line()
        .await
        .expect("IO error reading line")
        .expect("no line received");
    serde_json::from_str(&line).expect("failed to parse JSON message")
}

// ---------------------------------------------------------------------------
// 1. Connection to non-existent socket returns KrillError::Connection
// ---------------------------------------------------------------------------
#[tokio::test]
async fn connect_to_nonexistent_socket_returns_connection_error() {
    let result = KrillClient::connect(
        "test-service",
        std::path::PathBuf::from("/tmp/krill_nonexistent_socket_that_does_not_exist.sock"),
    )
    .await;

    match result {
        Ok(_) => panic!("Expected an error when connecting to a non-existent socket"),
        Err(KrillError::Connection(msg)) => {
            assert!(
                !msg.is_empty(),
                "Connection error message should not be empty"
            );
        }
        Err(other) => panic!("Expected KrillError::Connection, got: {}", other),
    }
}

// ---------------------------------------------------------------------------
// 2. Connection to real socket and heartbeat
// ---------------------------------------------------------------------------
#[tokio::test]
async fn connect_and_send_heartbeat() {
    let tmp_dir = TempDir::new().expect("failed to create temp dir");
    let socket_path = tmp_dir.path().join("krill_test.sock");
    let listener = UnixListener::bind(&socket_path).expect("failed to bind unix listener");

    let client = KrillClient::connect("my-service", socket_path.clone())
        .await
        .expect("failed to connect client");

    // Accept the connection on the server side, then have the client send.
    let server_handle = tokio::spawn(async move { accept_and_read_message(&listener).await });

    client.heartbeat().await.expect("heartbeat failed");

    let message = server_handle.await.expect("server task panicked");

    match message {
        ClientMessage::Heartbeat {
            service,
            status,
            metadata,
        } => {
            assert_eq!(service, "my-service");
            assert_eq!(status, ServiceStatus::Healthy);
            assert!(
                metadata.is_empty(),
                "expected empty metadata for plain heartbeat"
            );
        }
        other => panic!("Expected Heartbeat message, got: {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// 3. heartbeat_with_metadata
// ---------------------------------------------------------------------------
#[tokio::test]
async fn heartbeat_with_metadata_includes_metadata_in_json() {
    let tmp_dir = TempDir::new().expect("failed to create temp dir");
    let socket_path = tmp_dir.path().join("krill_test.sock");
    let listener = UnixListener::bind(&socket_path).expect("failed to bind unix listener");

    let client = KrillClient::connect("meta-service", socket_path.clone())
        .await
        .expect("failed to connect client");

    let server_handle = tokio::spawn(async move { accept_and_read_message(&listener).await });

    let mut metadata = HashMap::new();
    metadata.insert("version".to_string(), "1.2.3".to_string());
    metadata.insert("region".to_string(), "us-east-1".to_string());

    client
        .heartbeat_with_metadata(metadata.clone())
        .await
        .expect("heartbeat_with_metadata failed");

    let message = server_handle.await.expect("server task panicked");

    match message {
        ClientMessage::Heartbeat {
            service,
            status,
            metadata: received_meta,
        } => {
            assert_eq!(service, "meta-service");
            assert_eq!(status, ServiceStatus::Healthy);
            assert_eq!(
                received_meta.get("version").map(String::as_str),
                Some("1.2.3")
            );
            assert_eq!(
                received_meta.get("region").map(String::as_str),
                Some("us-east-1")
            );
        }
        other => panic!("Expected Heartbeat message, got: {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// 4. report_degraded
// ---------------------------------------------------------------------------
#[tokio::test]
async fn report_degraded_sends_degraded_status_with_reason() {
    let tmp_dir = TempDir::new().expect("failed to create temp dir");
    let socket_path = tmp_dir.path().join("krill_test.sock");
    let listener = UnixListener::bind(&socket_path).expect("failed to bind unix listener");

    let client = KrillClient::connect("degraded-service", socket_path.clone())
        .await
        .expect("failed to connect client");

    let server_handle = tokio::spawn(async move { accept_and_read_message(&listener).await });

    client
        .report_degraded("high memory usage")
        .await
        .expect("report_degraded failed");

    let message = server_handle.await.expect("server task panicked");

    match message {
        ClientMessage::Heartbeat {
            service,
            status,
            metadata,
        } => {
            assert_eq!(service, "degraded-service");
            assert_eq!(status, ServiceStatus::Degraded);
            assert_eq!(
                metadata.get("reason").map(String::as_str),
                Some("high memory usage"),
                "expected 'reason' key in metadata with the degraded reason"
            );
        }
        other => panic!("Expected Heartbeat message, got: {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// 5. report_healthy
// ---------------------------------------------------------------------------
#[tokio::test]
async fn report_healthy_sends_healthy_status() {
    let tmp_dir = TempDir::new().expect("failed to create temp dir");
    let socket_path = tmp_dir.path().join("krill_test.sock");
    let listener = UnixListener::bind(&socket_path).expect("failed to bind unix listener");

    let client = KrillClient::connect("healthy-service", socket_path.clone())
        .await
        .expect("failed to connect client");

    let server_handle = tokio::spawn(async move { accept_and_read_message(&listener).await });

    client
        .report_healthy()
        .await
        .expect("report_healthy failed");

    let message = server_handle.await.expect("server task panicked");

    match message {
        ClientMessage::Heartbeat {
            service,
            status,
            metadata,
        } => {
            assert_eq!(service, "healthy-service");
            assert_eq!(status, ServiceStatus::Healthy);
            assert!(
                metadata.is_empty(),
                "expected empty metadata for report_healthy"
            );
        }
        other => panic!("Expected Heartbeat message, got: {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// 6. KrillError display messages are descriptive
// ---------------------------------------------------------------------------
#[tokio::test]
async fn krill_error_display_is_descriptive() {
    // Connection error
    let conn_err = KrillError::Connection("refused".to_string());
    let display = format!("{}", conn_err);
    assert!(
        display.contains("Connection error"),
        "Connection error display should mention 'Connection error', got: {}",
        display,
    );
    assert!(
        display.contains("refused"),
        "Connection error display should contain the inner message, got: {}",
        display,
    );

    // IO error
    let io_err = KrillError::Io(std::io::Error::new(
        std::io::ErrorKind::BrokenPipe,
        "broken pipe",
    ));
    let display = format!("{}", io_err);
    assert!(
        display.contains("IO error"),
        "IO error display should mention 'IO error', got: {}",
        display,
    );
    assert!(
        display.contains("broken pipe"),
        "IO error display should contain the inner message, got: {}",
        display,
    );

    // Serialization error
    let ser_err = KrillError::Serialization("invalid utf-8".to_string());
    let display = format!("{}", ser_err);
    assert!(
        display.contains("Serialization error"),
        "Serialization error display should mention 'Serialization error', got: {}",
        display,
    );
    assert!(
        display.contains("invalid utf-8"),
        "Serialization error display should contain the inner message, got: {}",
        display,
    );
}

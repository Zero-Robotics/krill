use std::path::PathBuf;
use tempfile::TempDir;

use krill_cli::daemon_manager::is_daemon_running;

#[tokio::test]
async fn is_daemon_running_with_nonexistent_socket_returns_false() {
    let path = PathBuf::from("/tmp/krill_test_nonexistent_socket_99999.sock");
    // Make sure the file does not exist.
    let _ = std::fs::remove_file(&path);
    assert!(!path.exists());

    let running = is_daemon_running(&path).await;
    assert!(!running, "expected false for non-existent socket");
}

#[tokio::test]
async fn is_daemon_running_with_stale_socket_returns_false_and_cleans_up() {
    let tmp_dir = TempDir::new().expect("failed to create temp dir");
    let socket_path = tmp_dir.path().join("stale.sock");

    // Create a regular file at the socket path (not a real Unix socket).
    std::fs::write(&socket_path, b"not a real socket").expect("failed to write fake socket file");
    assert!(socket_path.exists(), "fake socket file should exist");

    let running = is_daemon_running(&socket_path).await;
    assert!(!running, "expected false for stale/fake socket");

    // The function should have removed the stale socket file.
    assert!(
        !socket_path.exists(),
        "stale socket file should have been cleaned up"
    );
}

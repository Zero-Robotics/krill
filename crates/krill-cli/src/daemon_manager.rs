// Daemon lifecycle management

use anyhow::{anyhow, Context, Result};
use krill_daemon::StartupMessage;
use std::os::fd::{FromRawFd, RawFd};
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tracing::{debug, info, warn};

/// Check if daemon is running by attempting to connect to socket
pub async fn is_daemon_running(socket_path: &Path) -> bool {
    if !socket_path.exists() {
        return false;
    }

    // Try to connect
    match UnixStream::connect(socket_path).await {
        Ok(_) => true,
        Err(_) => {
            // Socket exists but can't connect - stale socket
            warn!("Stale socket detected at {:?}, will clean up", socket_path);
            let _ = std::fs::remove_file(socket_path);
            false
        }
    }
}

/// Start daemon in background
pub async fn start_daemon_background(
    config_path: &Path,
    socket_path: &Path,
    log_dir: Option<&Path>,
) -> Result<()> {
    info!("Starting daemon in background...");

    use std::os::fd::AsRawFd;

    // Create pipe for startup communicaiton
    let (read_fd, write_fd) = os_pipe::pipe().context("Failed to create pipe")?;

    // Get current executable path
    let current_exe = std::env::current_exe()?;

    // Build command
    let mut cmd = tokio::process::Command::new(&current_exe);
    cmd.arg("daemon")
        .arg("--config")
        .arg(config_path)
        .arg("--socket")
        .arg(socket_path)
        .arg("--startup-pipe-fd")
        .arg(write_fd.as_raw_fd().to_string());

    if let Some(log_dir) = log_dir {
        cmd.arg("--log-dir").arg(log_dir);
    }

    // Detach from parent process
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    let child = cmd.spawn().context("Failed to spawn daemon process")?;

    // Detach - don't wait for child
    drop(child);

    // Close write end in parent (daemon holds it)
    drop(write_fd);

    let result = read_startup_result(read_fd.as_raw_fd()).await?;

    match result {
        StartupMessage::Success => {
            info!("Daemon spawned successfully");
            return Ok(());
        }
        StartupMessage::Error(e) => return Err(anyhow!("Daemon startup failed: {}", e)),
    }
}

pub async fn read_startup_result(read_fd: RawFd) -> Result<StartupMessage> {
    let file = unsafe { tokio::fs::File::from_raw_fd(read_fd) };
    let mut reader = tokio::io::BufReader::new(file);
    let mut line = String::new();

    match tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line)).await {
        Ok(Ok(_)) => serde_json::from_str(&line).context("Invalid startup message from daemon"),
        Ok(Err(e)) => Err(anyhow!("Failed to read from startup pipe: {}", e)),
        Err(_) => Err(anyhow!(
            "Daemon initialisation timed out - waiting for startup message"
        )),
    }
}

/// Wait for socket to become available with exponential backoff
pub async fn wait_for_socket(socket_path: &Path, timeout: Duration) -> Result<()> {
    let start = Instant::now();
    let mut delay = Duration::from_millis(10);

    debug!("Waiting for socket at {:?}", socket_path);

    while start.elapsed() < timeout {
        if socket_path.exists() {
            match UnixStream::connect(socket_path).await {
                Ok(_) => {
                    info!("Socket ready at {:?}", socket_path);
                    return Ok(());
                }
                Err(e) => {
                    debug!("Socket exists but not connectable yet: {}", e);
                }
            }
        }

        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(Duration::from_millis(500));
    }

    Err(anyhow!(
        "Daemon failed to start within {:?}. Check logs for errors.",
        timeout
    ))
}

/// Send a command to the daemon and wait for response
pub async fn send_command(
    socket_path: &Path,
    command: krill_common::ClientMessage,
) -> Result<krill_common::ServerMessage> {
    let stream = UnixStream::connect(socket_path)
        .await
        .context("Failed to connect to daemon")?;

    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);

    // Send command
    let json = serde_json::to_string(&command)?;
    writer
        .write_all(format!("{}\n", json).as_bytes())
        .await
        .context("Failed to send command")?;

    // Read response
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .await
        .context("Failed to read response")?;

    let response: krill_common::ServerMessage =
        serde_json::from_str(line.trim()).context("Failed to parse response")?;

    Ok(response)
}

/// Stop the daemon gracefully
pub async fn stop_daemon(socket_path: &Path) -> Result<()> {
    info!("Stopping daemon...");

    let command = krill_common::ClientMessage::Command {
        action: krill_common::CommandAction::StopDaemon,
        target: None,
    };

    send_command(socket_path, command).await?;

    // Wait for socket to be removed
    let start = Instant::now();
    while socket_path.exists() && start.elapsed() < Duration::from_secs(5) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    if socket_path.exists() {
        warn!("Socket still exists after shutdown, removing manually");
        std::fs::remove_file(socket_path)?;
    }

    info!("Daemon stopped successfully");
    Ok(())
}

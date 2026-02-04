use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use nix::sys::signal::{Signal, killpg};
use nix::unistd::{Pid, setpgid};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::Command;
use tokio::sync::{Mutex, RwLock, mpsc};
use tracing::{error, info, warn};

use crate::daemon::DaemonEvent;

/// Ring buffer for storing process output lines
pub struct RingBuffer<T> {
    buffer: VecDeque<T>,
    capacity: usize,
}

impl<T> RingBuffer<T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    pub fn push(&mut self, item: T) {
        if self.buffer.len() >= self.capacity {
            self.buffer.pop_front();
        }
        self.buffer.push_back(item);
    }

    pub fn get_lines(&self) -> Vec<T>
    where
        T: Clone,
    {
        self.buffer.iter().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
    }
}

/// Output lines from a process
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessOutputLine {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub stream: OutputStream,
    pub line: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OutputStream {
    Stdout,
    Stderr,
}

/// Handle to a running process
pub struct ProcessHandle {
    pub pid: u32,
    pub pgid: i32,
    pub stdout_task: tokio::task::JoinHandle<()>,
    pub stderr_task: tokio::task::JoinHandle<()>,
    pub monitor_task: tokio::task::JoinHandle<()>,
}

/// Manager for all processes
pub struct ProcessManager {
    log_dir: PathBuf,
    processes: Arc<Mutex<std::collections::HashMap<String, ProcessHandle>>>,
    output_buffers: Arc<RwLock<std::collections::HashMap<String, RingBuffer<ProcessOutputLine>>>>,
    event_tx: mpsc::Sender<DaemonEvent>,
}

impl ProcessManager {
    /// Create a new process manager
    pub fn new(log_dir: PathBuf, event_tx: mpsc::Sender<DaemonEvent>) -> Result<Self, String> {
        // Create log directory
        std::fs::create_dir_all(&log_dir)
            .map_err(|e| format!("Failed to create log directory: {}", e))?;

        Ok(Self {
            log_dir,
            processes: Arc::new(Mutex::new(std::collections::HashMap::new())),
            output_buffers: Arc::new(RwLock::new(std::collections::HashMap::new())),
            event_tx,
        })
    }

    /// Spawn a new process
    pub async fn spawn_process(
        &self,
        service_name: String,
        command: String,
        environment: Option<std::collections::HashMap<String, String>>,
        working_directory: Option<String>,
    ) -> Result<u32, String> {
        info!(
            "Spawning process for service '{}': {}",
            service_name, command
        );

        // Parse command string using proper shell word splitting
        let parts = shell_words::split(&command)
            .map_err(|e| format!("Failed to parse command '{}': {}", command, e))?;

        if parts.is_empty() {
            return Err("Empty command".to_string());
        }

        let executable = &parts[0];
        let args = &parts[1..];

        // Create command using tokio::process::Command
        let mut cmd = Command::new(executable);
        cmd.args(args);

        // Set environment variables
        if let Some(env) = environment {
            cmd.envs(env);
        }

        // Set working directory
        if let Some(working_dir) = working_directory {
            cmd.current_dir(working_dir);
        }

        // Redirect output to pipes
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        // Spawn the process
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn process: {}", e))?;

        // Get PID
        let pid = child.id().ok_or("Failed to get process ID".to_string())?;

        // Set process group (PGID) manually using nix
        // We'll create a new process group with the PID as PGID
        if let Err(e) = setpgid(Pid::from_raw(pid as i32), Pid::from_raw(pid as i32)) {
            warn!("Failed to set process group for PID {}: {}", pid, e);
        }

        // Get process group ID (negative PID for killing group)
        let pgid = -(pid as i32);

        info!(
            "Process spawned for service '{}' with PID {} (PGID {})",
            service_name, pid, pgid
        );

        // Create output buffers
        {
            let mut buffers = self.output_buffers.write().await;
            buffers.insert(service_name.clone(), RingBuffer::new(500)); // Last 500 lines
        }

        // Capture stdout
        let stdout = child
            .stdout
            .take()
            .ok_or("Failed to capture stdout".to_string())?;
        let stdout_service = service_name.clone();
        let stdout_buffers = self.output_buffers.clone();
        let stdout_event_tx = self.event_tx.clone();

        let stdout_task = tokio::spawn(async move {
            Self::capture_output(
                stdout,
                OutputStream::Stdout,
                stdout_service,
                stdout_buffers,
                stdout_event_tx,
            )
            .await;
        });

        // Capture stderr
        let stderr = child
            .stderr
            .take()
            .ok_or("Failed to capture stderr".to_string())?;
        let stderr_service = service_name.clone();
        let stderr_buffers = self.output_buffers.clone();
        let stderr_event_tx = self.event_tx.clone();

        let stderr_task = tokio::spawn(async move {
            Self::capture_output(
                stderr,
                OutputStream::Stderr,
                stderr_service,
                stderr_buffers,
                stderr_event_tx,
            )
            .await;
        });

        // Monitor process exit
        let service_name_monitor = service_name.clone();
        let pid_monitor = pid;
        let event_tx_monitor = self.event_tx.clone();
        let buffers_monitor = self.output_buffers.clone();
        let processes_monitor = self.processes.clone();

        let monitor_task = tokio::spawn(async move {
            let exit_result = child.wait().await;

            // Clean up from process map
            let mut processes = processes_monitor.lock().await;
            processes.remove(&service_name_monitor);

            // Process exit result
            match exit_result {
                Ok(status) => {
                    let exit_code = status.code();

                    // Log to buffer
                    let mut buffers = buffers_monitor.write().await;
                    if let Some(buffer) = buffers.get_mut(&service_name_monitor) {
                        let message = match exit_code {
                            Some(code) => format!("Process exited with code: {}", code),
                            None => "Process terminated by signal".to_string(),
                        };
                        buffer.push(ProcessOutputLine {
                            timestamp: chrono::Utc::now(),
                            stream: OutputStream::Stdout,
                            line: message,
                        });
                    }

                    // Send event
                    let _ = event_tx_monitor
                        .send(DaemonEvent::ServiceStopped(
                            service_name_monitor.clone(),
                            exit_code,
                        ))
                        .await;

                    if let Some(code) = exit_code {
                        if code != 0 {
                            let _ = event_tx_monitor
                                .send(DaemonEvent::ServiceFailed(
                                    service_name_monitor,
                                    Some(code),
                                    format!("Process exited with non-zero code: {}", code),
                                ))
                                .await;
                        }
                    }
                }
                Err(e) => {
                    error!("Error waiting for process {}: {}", pid_monitor, e);

                    let _ = event_tx_monitor
                        .send(DaemonEvent::ServiceFailed(
                            service_name_monitor,
                            None,
                            format!("Process error: {}", e),
                        ))
                        .await;
                }
            }
        });

        // Store process handle
        let handle = ProcessHandle {
            pid,
            pgid,
            stdout_task,
            stderr_task,
            monitor_task,
        };

        let mut processes = self.processes.lock().await;
        processes.insert(service_name.clone(), handle);

        // Send startup event
        let _ = self
            .event_tx
            .send(DaemonEvent::ServiceStarted(service_name, pid))
            .await;

        Ok(pid)
    }

    /// Capture output from a stream and buffer it
    async fn capture_output<R>(
        stream: R,
        stream_type: OutputStream,
        service_name: String,
        buffers: Arc<RwLock<std::collections::HashMap<String, RingBuffer<ProcessOutputLine>>>>,
        event_tx: mpsc::Sender<DaemonEvent>,
    ) where
        R: AsyncRead + Unpin,
    {
        let mut reader = BufReader::new(stream);
        let mut line = String::new();

        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => break, // EOF
                Ok(_) => {
                    let line = line.trim_end().to_string();
                    if line.is_empty() {
                        continue;
                    }

                    let timestamp = chrono::Utc::now();
                    let output_line = ProcessOutputLine {
                        timestamp,
                        stream: stream_type.clone(),
                        line: line.clone(),
                    };

                    // Store in buffer
                    {
                        let mut buffers = buffers.write().await;
                        if let Some(buffer) = buffers.get_mut(&service_name) {
                            buffer.push(output_line.clone());
                        } else {
                            // Buffer was removed, stop capturing
                            break;
                        }
                    }

                    // Send log event
                    let _ = event_tx
                        .send(DaemonEvent::LogMessage {
                            service: Some(service_name.clone()),
                            level: match stream_type {
                                OutputStream::Stdout => tracing::Level::INFO,
                                OutputStream::Stderr => tracing::Level::ERROR,
                            },
                            message: line,
                        })
                        .await;
                }
                Err(e) => {
                    warn!("Error reading from stream: {}", e);
                    break;
                }
            }
        }
    }

    /// Stop a process by service name
    pub async fn stop_process(&self, service_name: &str) -> Result<(), String> {
        info!("Stopping process for service '{}'", service_name);

        let handle = {
            let mut processes = self.processes.lock().await;
            processes.remove(service_name)
        };

        if let Some(handle) = handle {
            // Send SIGTERM to the entire process group
            match killpg(Pid::from_raw(handle.pgid), Signal::SIGTERM) {
                Ok(_) => {
                    info!(
                        "Sent SIGTERM to process group {} for service '{}'",
                        handle.pgid, service_name
                    );

                    // Wait for monitor task to complete (process exit) with timeout
                    let wait_result = tokio::time::timeout(
                        std::time::Duration::from_secs(10),
                        handle.monitor_task,
                    )
                    .await;

                    match wait_result {
                        Ok(Ok(_)) => {
                            info!(
                                "Process for service '{}' terminated gracefully",
                                service_name
                            );
                        }
                        Ok(Err(e)) => {
                            warn!("Monitor task error for '{}': {}", service_name, e);
                            // Fall back to SIGKILL
                            self.kill_process_group(handle.pgid).await?;
                        }
                        Err(_) => {
                            warn!(
                                "Timeout waiting for process '{}' to terminate, sending SIGKILL",
                                service_name
                            );
                            self.kill_process_group(handle.pgid).await?;
                        }
                    }
                }
                Err(e) => {
                    warn!(
                        "Failed to send SIGTERM to process group {}: {}",
                        handle.pgid, e
                    );
                    // Try SIGKILL directly
                    self.kill_process_group(handle.pgid).await?;
                }
            }

            // Cancel output tasks
            handle.stdout_task.abort();
            handle.stderr_task.abort();

            Ok(())
        } else {
            Err(format!("Process for service '{}' not found", service_name))
        }
    }

    /// Kill a process group with SIGKILL
    async fn kill_process_group(&self, pgid: i32) -> Result<(), String> {
        match killpg(Pid::from_raw(pgid), Signal::SIGKILL) {
            Ok(_) => {
                info!("Sent SIGKILL to process group {}", pgid);
                Ok(())
            }
            Err(e) => {
                error!("Failed to send SIGKILL to process group {}: {}", pgid, e);
                Err(format!("Failed to kill process group {}: {}", pgid, e))
            }
        }
    }

    /// Stop all processes
    pub async fn stop_all(&self) -> Result<(), String> {
        info!("Stopping all processes");

        let processes = self.processes.lock().await;
        let service_names: Vec<String> = processes.keys().cloned().collect();

        drop(processes); // Release lock before async operations

        let mut errors = Vec::new();

        for service_name in service_names {
            if let Err(e) = self.stop_process(&service_name).await {
                errors.push(format!("{}: {}", service_name, e));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join(", "))
        }
    }

    /// Get process output
    pub async fn get_output(
        &self,
        service_name: &str,
        lines: usize,
    ) -> Result<Vec<ProcessOutputLine>, String> {
        let buffers = self.output_buffers.read().await;

        if let Some(buffer) = buffers.get(service_name) {
            let all_lines = buffer.get_lines();
            let start = if all_lines.len() > lines {
                all_lines.len() - lines
            } else {
                0
            };
            Ok(all_lines[start..].to_vec())
        } else {
            Err(format!(
                "No output buffer found for service '{}'",
                service_name
            ))
        }
    }

    /// Check if a process is running
    pub async fn is_running(&self, service_name: &str) -> bool {
        let processes = self.processes.lock().await;
        processes.contains_key(service_name)
    }

    /// Get process PID
    pub async fn get_pid(&self, service_name: &str) -> Option<u32> {
        let processes = self.processes.lock().await;
        processes.get(service_name).map(|h| h.pid)
    }

    /// Get all running processes
    pub async fn get_running_processes(&self) -> Vec<String> {
        let processes = self.processes.lock().await;
        processes.keys().cloned().collect()
    }

    /// Clear output buffer for a service
    pub async fn clear_output(&self, service_name: &str) -> Result<(), String> {
        let mut buffers = self.output_buffers.write().await;

        if let Some(buffer) = buffers.get_mut(service_name) {
            buffer.clear();
            Ok(())
        } else {
            Err(format!(
                "No output buffer found for service '{}'",
                service_name
            ))
        }
    }

    /// Get log file path for a service
    pub fn get_log_file_path(&self, service_name: &str) -> PathBuf {
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        self.log_dir
            .join(format!("{}_{}.log", service_name, timestamp))
    }
}

impl Clone for ProcessManager {
    fn clone(&self) -> Self {
        Self {
            log_dir: self.log_dir.clone(),
            processes: self.processes.clone(),
            output_buffers: self.output_buffers.clone(),
            event_tx: self.event_tx.clone(),
        }
    }
}

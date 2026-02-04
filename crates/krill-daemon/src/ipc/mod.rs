use std::path::PathBuf;
use std::sync::Arc;

use krill_common::model::{HeartbeatMessage, Message, RequestMessage, ResponseMessage};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Mutex, RwLock, mpsc};
use tracing::{debug, error, info, warn};

use crate::daemon::{Daemon, DaemonCommand, DaemonEvent};

/// IPC server for Unix domain socket communication
pub struct IpcServer {
    /// Path to the Unix socket
    socket_path: PathBuf,
    /// Daemon instance for command processing
    daemon: Arc<Daemon>,
    /// Event receiver for broadcasting events to clients
    event_rx: mpsc::Receiver<DaemonEvent>,
    /// Connected clients
    clients: Arc<Mutex<Vec<mpsc::Sender<Message>>>>,
    /// Shutdown signal
    shutdown: Arc<Mutex<bool>>,
}

/// Handle for controlling the IPC server
pub struct IpcServerHandle {
    /// Task handle for the server
    task: tokio::task::JoinHandle<()>,
    /// Shutdown signal sender
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
}

impl IpcServer {
    /// Create a new IPC server
    pub async fn new(socket_path: PathBuf, daemon: Arc<Daemon>) -> Result<Self, String> {
        // Remove existing socket if it exists
        if socket_path.exists() {
            std::fs::remove_file(&socket_path)
                .map_err(|e| format!("Failed to remove existing socket: {}", e))?;
        }

        // Create parent directory if needed
        if let Some(parent) = socket_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create socket directory: {}", e))?;
        }

        // Get event receiver from daemon
        let event_rx = daemon
            .take_event_receiver()
            .await
            .map_err(|e| format!("Failed to get event receiver: {}", e))?;

        Ok(Self {
            socket_path,
            daemon,
            event_rx,
            clients: Arc::new(Mutex::new(Vec::new())),
            shutdown: Arc::new(Mutex::new(false)),
        })
    }

    /// Start the IPC server
    pub async fn start(self) -> Result<IpcServerHandle, String> {
        info!("Starting IPC server on {}", self.socket_path.display());

        // Create Unix socket listener
        let listener = UnixListener::bind(&self.socket_path)
            .map_err(|e| format!("Failed to bind Unix socket: {}", e))?;

        info!("IPC server listening on {}", self.socket_path.display());

        let arc_self = Arc::new(self);
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        // Spawn server task
        let task = tokio::spawn(async move {
            let server = arc_self.clone();

            // Accept connections loop
            tokio::select! {
                _ = server.accept_connections(listener) => {
                    debug!("Accept connections loop ended");
                }
                _ = shutdown_rx => {
                    info!("IPC server received shutdown signal");
                }
            }

            // Cleanup
            if let Err(e) = arc_self.cleanup().await {
                error!("Error during IPC server cleanup: {}", e);
            }
        });

        Ok(IpcServerHandle { task, shutdown_tx })
    }

    /// Main connection acceptance loop
    async fn accept_connections(self: Arc<Self>, listener: UnixListener) {
        info!("IPC server ready to accept connections");

        loop {
            // Check for shutdown
            {
                let shutdown = self.shutdown.lock().await;
                if *shutdown {
                    break;
                }
            }

            // Accept next connection
            match listener.accept().await {
                Ok((stream, _addr)) => {
                    debug!("New IPC connection accepted");

                    // Spawn handler for this connection
                    let server = self.clone();
                    tokio::spawn(async move {
                        if let Err(e) = server.handle_connection(stream).await {
                            warn!("Error handling IPC connection: {}", e);
                        }
                    });
                }
                Err(e) => {
                    error!("Error accepting connection: {}", e);
                    // Don't break on accept errors, continue listening
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
            }
        }
    }

    /// Handle a single client connection
    async fn handle_connection(self: Arc<Self>, stream: UnixStream) -> Result<(), String> {
        let (read_half, write_half) = tokio::io::split(stream);
        let reader = BufReader::new(read_half);
        let writer = BufWriter::new(write_half);

        // Create channel for sending messages to this client
        let (message_tx, mut message_rx) = mpsc::channel::<Message>(10);

        // Register client
        {
            let mut clients = self.clients.lock().await;
            clients.push(message_tx.clone());
        }

        // Spawn writer task
        let writer_task = tokio::spawn(Self::writer_task(writer, message_rx));

        // Read and process messages
        let mut lines = reader.lines();
        while let Some(line) = lines
            .next_line()
            .await
            .map_err(|e| format!("Read error: {}", e))?
        {
            // Check for shutdown
            {
                let shutdown = self.shutdown.lock().await;
                if *shutdown {
                    break;
                }
            }

            // Parse and handle message
            match self.handle_message(&line, message_tx.clone()).await {
                Ok(_) => debug!("Message handled successfully"),
                Err(e) => {
                    warn!("Error handling message: {}", e);
                    // Send error response
                    if let Err(send_err) = self
                        .send_error_response(&line, &e, message_tx.clone())
                        .await
                    {
                        warn!("Failed to send error response: {}", send_err);
                    }
                }
            }
        }

        // Clean up
        {
            let mut clients = self.clients.lock().await;
            clients.retain(|tx| !tx.is_closed());
        }

        // Wait for writer task to finish
        writer_task.abort();

        debug!("IPC connection closed");
        Ok(())
    }

    /// Handle a single message from a client
    async fn handle_message(
        &self,
        line: &str,
        response_tx: mpsc::Sender<Message>,
    ) -> Result<(), String> {
        debug!("Received message: {}", line);

        // Parse JSON message
        let message: Message = serde_json::from_str(line)
            .map_err(|e| format!("Failed to parse JSON message: {}", e))?;

        match message {
            Message::Heartbeat(heartbeat) => {
                self.handle_heartbeat(heartbeat).await?;
                // Heartbeats don't require a response
            }
            Message::Request(request) => {
                let response = self.handle_request(request).await?;
                let response_msg = Message::Response(response);

                // Send response back to client
                response_tx
                    .send(response_msg)
                    .await
                    .map_err(|e| format!("Failed to send response: {}", e))?;
            }
            Message::Response(_) => {
                return Err("Server should not receive Response messages".to_string());
            }
            Message::Event(_) => {
                return Err("Server should not receive Event messages from clients".to_string());
            }
        }

        Ok(())
    }

    /// Handle a heartbeat message from SDK
    async fn handle_heartbeat(&self, heartbeat: HeartbeatMessage) -> Result<(), String> {
        debug!("Processing heartbeat for service '{}'", heartbeat.service);

        // Record heartbeat in health monitor
        self.daemon
            .health_monitor()
            .record_heartbeat(&heartbeat.service)
            .await
            .map_err(|e| format!("Failed to record heartbeat: {}", e))?;

        // Update service state if needed
        if matches!(heartbeat.status, krill_common::model::ServiceState::Healthy) {
            self.daemon
                .update_service_state(&heartbeat.service, heartbeat.status, None, None)
                .await
                .map_err(|e| format!("Failed to update service state: {}", e))?;
        }

        Ok(())
    }

    /// Handle a request message from TUI
    async fn handle_request(&self, request: RequestMessage) -> Result<ResponseMessage, String> {
        debug!(
            "Processing request: {} (id: {})",
            request.method, request.id
        );

        match request.method.as_str() {
            "start_service" => self.handle_start_service(request).await,
            "stop_service" => self.handle_stop_service(request).await,
            "restart_service" => self.handle_restart_service(request).await,
            "emergency_stop" => self.handle_emergency_stop(request).await,
            "get_status" => self.handle_get_status(request).await,
            "get_service_status" => self.handle_get_service_status(request).await,
            "get_service_logs" => self.handle_get_service_logs(request).await,
            "list_services" => self.handle_list_services(request).await,
            "health_check" => self.handle_health_check(request).await,
            "clear_safety_stop" => self.handle_clear_safety_stop(request).await,
            _ => Err(format!("Unknown method: {}", request.method)),
        }
    }

    /// Handle start_service request
    async fn handle_start_service(
        &self,
        request: RequestMessage,
    ) -> Result<ResponseMessage, String> {
        let params: serde_json::Value = request
            .params
            .ok_or_else(|| "Missing parameters".to_string())?;

        let service_name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing 'name' parameter".to_string())?;

        // Send command to daemon
        self.daemon
            .command_sender()
            .send(DaemonCommand::StartService(service_name.to_string()))
            .await
            .map_err(|e| format!("Failed to send start command: {}", e))?;

        Ok(ResponseMessage::success(
            request.id,
            Some(serde_json::json!({"status": "accepted"})),
        ))
    }

    /// Handle stop_service request
    async fn handle_stop_service(
        &self,
        request: RequestMessage,
    ) -> Result<ResponseMessage, String> {
        let params: serde_json::Value = request
            .params
            .ok_or_else(|| "Missing parameters".to_string())?;

        let service_name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing 'name' parameter".to_string())?;

        self.daemon
            .command_sender()
            .send(DaemonCommand::StopService(service_name.to_string()))
            .await
            .map_err(|e| format!("Failed to send stop command: {}", e))?;

        Ok(ResponseMessage::success(
            request.id,
            Some(serde_json::json!({"status": "accepted"})),
        ))
    }

    /// Handle restart_service request
    async fn handle_restart_service(
        &self,
        request: RequestMessage,
    ) -> Result<ResponseMessage, String> {
        let params: serde_json::Value = request
            .params
            .ok_or_else(|| "Missing parameters".to_string())?;

        let service_name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing 'name' parameter".to_string())?;

        self.daemon
            .command_sender()
            .send(DaemonCommand::RestartService(service_name.to_string()))
            .await
            .map_err(|e| format!("Failed to send restart command: {}", e))?;

        Ok(ResponseMessage::success(
            request.id,
            Some(serde_json::json!({"status": "accepted"})),
        ))
    }

    /// Handle emergency_stop request
    async fn handle_emergency_stop(
        &self,
        request: RequestMessage,
    ) -> Result<ResponseMessage, String> {
        self.daemon
            .command_sender()
            .send(DaemonCommand::EmergencyStop)
            .await
            .map_err(|e| format!("Failed to send emergency stop command: {}", e))?;

        Ok(ResponseMessage::success(
            request.id,
            Some(serde_json::json!({"status": "emergency_stop_triggered"})),
        ))
    }

    /// Handle get_status request
    async fn handle_get_status(&self, request: RequestMessage) -> Result<ResponseMessage, String> {
        let service_states = self.daemon.get_service_states().await;

        let status = serde_json::json!({
            "session_id": self.daemon.session_id(),
            "emergency_mode": self.daemon.is_emergency_mode().await,
            "services": service_states,
        });

        Ok(ResponseMessage::success(request.id, Some(status)))
    }

    /// Handle get_service_status request
    async fn handle_get_service_status(
        &self,
        request: RequestMessage,
    ) -> Result<ResponseMessage, String> {
        let params: serde_json::Value = request
            .params
            .ok_or_else(|| "Missing parameters".to_string())?;

        let service_name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing 'name' parameter".to_string())?;

        let service_states = self.daemon.get_service_states().await;

        if let Some(status) = service_states.get(service_name) {
            Ok(ResponseMessage::success(
                request.id,
                Some(serde_json::to_value(status).unwrap()),
            ))
        } else {
            Err(format!("Service '{}' not found", service_name))
        }
    }

    /// Handle get_service_logs request
    async fn handle_get_service_logs(
        &self,
        request: RequestMessage,
    ) -> Result<ResponseMessage, String> {
        let params: serde_json::Value = request
            .params
            .ok_or_else(|| "Missing parameters".to_string())?;

        let service_name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing 'name' parameter".to_string())?;

        let lines = params.get("lines").and_then(|v| v.as_u64()).unwrap_or(100) as usize;

        let logs = self
            .daemon
            .process_manager()
            .get_output(service_name, lines)
            .await
            .map_err(|e| format!("Failed to get logs: {}", e))?;

        Ok(ResponseMessage::success(
            request.id,
            Some(serde_json::to_value(logs).unwrap()),
        ))
    }

    /// Handle list_services request
    async fn handle_list_services(
        &self,
        request: RequestMessage,
    ) -> Result<ResponseMessage, String> {
        let config = self.daemon.config();
        let services: Vec<_> = config.services.keys().collect();

        Ok(ResponseMessage::success(
            request.id,
            Some(serde_json::to_value(services).unwrap()),
        ))
    }

    /// Handle health_check request
    async fn handle_health_check(
        &self,
        request: RequestMessage,
    ) -> Result<ResponseMessage, String> {
        let params: serde_json::Value = request
            .params
            .ok_or_else(|| "Missing parameters".to_string())?;

        let service_name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing 'name' parameter".to_string())?;

        let is_healthy = self
            .daemon
            .health_monitor()
            .is_service_healthy(service_name)
            .await;

        Ok(ResponseMessage::success(
            request.id,
            Some(serde_json::json!({"healthy": is_healthy})),
        ))
    }

    /// Handle clear_safety_stop request
    async fn handle_clear_safety_stop(
        &self,
        request: RequestMessage,
    ) -> Result<ResponseMessage, String> {
        let params: serde_json::Value = request
            .params
            .ok_or_else(|| "Missing parameters".to_string())?;

        let service_name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing 'name' parameter".to_string())?;

        self.daemon
            .safety_interceptor()
            .clear_safety_stopped(service_name)
            .await
            .map_err(|e| format!("Failed to clear safety stop: {}", e))?;

        Ok(ResponseMessage::success(
            request.id,
            Some(serde_json::json!({"status": "cleared"})),
        ))
    }

    /// Send error response for failed message handling
    async fn send_error_response(
        &self,
        original_line: &str,
        error: &str,
        response_tx: mpsc::Sender<Message>,
    ) -> Result<(), String> {
        // Try to extract request ID from original message
        let maybe_id = serde_json::from_str::<serde_json::Value>(original_line)
            .ok()
            .and_then(|v| v.get("id").cloned());

        if let Some(id) = maybe_id {
            let uuid = serde_json::from_value(id).unwrap_or_default();
            let response = ResponseMessage::error(
                uuid,
                400,
                error.to_string(),
                Some(serde_json::json!({"original_message": original_line})),
            );

            let response_msg = Message::Response(response);
            response_tx
                .send(response_msg)
                .await
                .map_err(|e| format!("Failed to send error response: {}", e))?;
        }

        Ok(())
    }

    /// Writer task for sending messages to client
    async fn writer_task(
        mut writer: tokio::io::BufWriter<tokio::io::WriteHalf<UnixStream>>,
        mut message_rx: mpsc::Receiver<Message>,
    ) {
        while let Some(message) = message_rx.recv().await {
            if let Err(e) = Self::write_message(&mut writer, &message).await {
                warn!("Failed to write message to client: {}", e);
                break;
            }
        }
    }

    /// Write a message to the writer
    async fn write_message(
        writer: &mut tokio::io::BufWriter<tokio::io::WriteHalf<UnixStream>>,
        message: &Message,
    ) -> Result<(), String> {
        let json = serde_json::to_string(message)
            .map_err(|e| format!("Failed to serialize message: {}", e))?;

        writer
            .write_all(json.as_bytes())
            .await
            .map_err(|e| format!("Write error: {}", e))?;

        writer
            .write_all(b"\n")
            .await
            .map_err(|e| format!("Write error: {}", e))?;

        writer
            .flush()
            .await
            .map_err(|e| format!("Flush error: {}", e))?;

        Ok(())
    }

    /// Broadcast an event to all connected clients
    pub async fn broadcast_event(&self, event: DaemonEvent) -> Result<(), String> {
        let message = match event {
            DaemonEvent::ServiceStateChanged {
                service,
                old_state,
                new_state,
                pid,
            } => Message::Event(krill_common::model::EventMessage::state_transition(
                service, old_state, new_state,
            )),
            DaemonEvent::ServiceStarted(service, pid) => {
                let mut metadata = std::collections::HashMap::new();
                metadata.insert("pid".to_string(), serde_json::json!(pid));

                Message::Event(krill_common::model::EventMessage {
                    version: "1.0".to_string(),
                    event: krill_common::model::EventType::ServiceStarted,
                    service: Some(service),
                    from: None,
                    to: None,
                    pid: Some(pid),
                    exit_code: None,
                    metadata: Some(metadata),
                    timestamp: chrono::Utc::now(),
                })
            }
            DaemonEvent::ServiceStopped(service, exit_code) => {
                let mut metadata = std::collections::HashMap::new();
                if let Some(code) = exit_code {
                    metadata.insert("exit_code".to_string(), serde_json::json!(code));
                }

                Message::Event(krill_common::model::EventMessage {
                    version: "1.0".to_string(),
                    event: krill_common::model::EventType::ServiceStopped,
                    service: Some(service),
                    from: None,
                    to: None,
                    pid: None,
                    exit_code,
                    metadata: Some(metadata),
                    timestamp: chrono::Utc::now(),
                })
            }
            DaemonEvent::ServiceFailed(service, exit_code, reason) => {
                let mut metadata = std::collections::HashMap::new();
                metadata.insert("reason".to_string(), serde_json::json!(reason));
                if let Some(code) = exit_code {
                    metadata.insert("exit_code".to_string(), serde_json::json!(code));
                }

                Message::Event(krill_common::model::EventMessage {
                    version: "1.0".to_string(),
                    event: krill_common::model::EventType::ServiceFailed,
                    service: Some(service),
                    from: None,
                    to: None,
                    pid: None,
                    exit_code,
                    metadata: Some(metadata),
                    timestamp: chrono::Utc::now(),
                })
            }
            DaemonEvent::CriticalFailure(service, reason) => {
                let mut metadata = std::collections::HashMap::new();
                metadata.insert("reason".to_string(), serde_json::json!(reason));

                Message::Event(krill_common::model::EventMessage::critical_failure(
                    service, None,
                ))
            }
            DaemonEvent::EmergencyStopTriggered(reason) => {
                let mut metadata = std::collections::HashMap::new();
                metadata.insert("reason".to_string(), serde_json::json!(reason));

                Message::Event(krill_common::model::EventMessage {
                    version: "1.0".to_string(),
                    event: krill_common::model::EventType::EmergencyStop,
                    service: None,
                    from: None,
                    to: None,
                    pid: None,
                    exit_code: None,
                    metadata: Some(metadata),
                    timestamp: chrono::Utc::now(),
                })
            }
            DaemonEvent::HeartbeatReceived(service) => {
                // Heartbeats are internal, not broadcast
                return Ok(());
            }
            DaemonEvent::DependencySatisfied(dependent, dependency) => {
                let mut metadata = std::collections::HashMap::new();
                metadata.insert("dependency".to_string(), serde_json::json!(dependency));

                Message::Event(krill_common::model::EventMessage {
                    version: "1.0".to_string(),
                    event: krill_common::model::EventType::StateTransition,
                    service: Some(dependent),
                    from: Some(krill_common::model::ServiceState::Starting),
                    to: Some(krill_common::model::ServiceState::Running),
                    pid: None,
                    exit_code: None,
                    metadata: Some(metadata),
                    timestamp: chrono::Utc::now(),
                })
            }
            DaemonEvent::LogMessage {
                service,
                level,
                message,
            } => {
                let mut metadata = std::collections::HashMap::new();
                metadata.insert("level".to_string(), serde_json::json!(level.to_string()));

                Message::Event(krill_common::model::EventMessage {
                    version: "1.0".to_string(),
                    event: krill_common::model::EventType::LogMessage,
                    service,
                    from: None,
                    to: None,
                    pid: None,
                    exit_code: None,
                    metadata: Some(metadata),
                    timestamp: chrono::Utc::now(),
                })
            }
        };

        let clients = self.clients.lock().await;
        let mut dead_clients = Vec::new();

        for (i, client_tx) in clients.iter().enumerate() {
            if client_tx.is_closed() {
                dead_clients.push(i);
                continue;
            }

            if let Err(e) = client_tx.send(message.clone()).await {
                warn!("Failed to broadcast to client {}: {}", i, e);
                dead_clients.push(i);
            }
        }

        // Clean up dead clients (outside lock to avoid deadlock)
        if !dead_clients.is_empty() {
            drop(clients);
            let mut clients = self.clients.lock().await;
            for &index in dead_clients.iter().rev() {
                if index < clients.len() {
                    clients.remove(index);
                }
            }
        }

        Ok(())
    }

    /// Clean up resources
    async fn cleanup(&self) -> Result<(), String> {
        info!("Cleaning up IPC server");

        // Remove socket file
        if self.socket_path.exists() {
            std::fs::remove_file(&self.socket_path)
                .map_err(|e| format!("Failed to remove socket file: {}", e))?;
            info!("Removed socket file: {}", self.socket_path.display());
        }

        Ok(())
    }

    /// Shutdown the IPC server
    pub async fn shutdown(&self) {
        info!("Shutting down IPC server");

        // Set shutdown flag
        {
            let mut shutdown = self.shutdown.lock().await;
            *shutdown = true;
        }

        // Close all client connections
        {
            let mut clients = self.clients.lock().await;
            clients.clear();
        }
    }
}

impl IpcServerHandle {
    /// Shutdown the IPC server
    pub async fn shutdown(self) -> Result<(), String> {
        info!("Shutting down IPC server via handle");

        // Send shutdown signal
        if let Err(_) = self.shutdown_tx.send(()) {
            warn!("IPC server already shut down");
        }

        // Wait for task to complete
        match self.task.await {
            Ok(_) => {
                info!("IPC server task completed successfully");
                Ok(())
            }
            Err(e) => {
                if e.is_panic() {
                    Err("IPC server task panicked".to_string())
                } else {
                    Err(format!("IPC server task cancelled: {:?}", e))
                }
            }
        }
    }
}

/// Start the IPC server (convenience function)
pub async fn start_ipc_server(daemon: Arc<Daemon>) -> Result<IpcServerHandle, String> {
    let socket_path = daemon.socket_path().clone();
    let ipc_server = IpcServer::new(socket_path, daemon).await?;
    ipc_server.start().await
}

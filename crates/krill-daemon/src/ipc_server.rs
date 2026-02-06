// IPC Server - Unix socket server for client communication

use crate::logging::LogStore;
use krill_common::ipc::ServiceSnapshot;
use krill_common::{ClientMessage, CommandAction, ServerMessage, ServiceStatus};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{broadcast, mpsc, Mutex};
use tracing::{debug, error, info, warn};

#[derive(Debug, Error)]
pub enum IpcError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Socket already exists: {0}")]
    SocketExists(PathBuf),

    #[error("Invalid message: {0}")]
    InvalidMessage(String),
}

pub type CommandRequest = (CommandAction, Option<String>);
pub type HeartbeatMessage = (String, ServiceStatus, HashMap<String, String>); // (service_name, status, metadata)

pub struct IpcServer {
    socket_path: PathBuf,
    event_broadcast: broadcast::Sender<ServerMessage>,
    command_tx: mpsc::UnboundedSender<CommandRequest>,
    snapshot_req_tx: mpsc::UnboundedSender<mpsc::UnboundedSender<HashMap<String, ServiceSnapshot>>>,
    heartbeat_tx: Option<mpsc::UnboundedSender<HeartbeatMessage>>,
    log_store: Option<Arc<LogStore>>,
    shutdown: Arc<Mutex<bool>>,
}

impl IpcServer {
    pub fn new(
        socket_path: PathBuf,
        command_tx: mpsc::UnboundedSender<CommandRequest>,
        snapshot_req_tx: mpsc::UnboundedSender<
            mpsc::UnboundedSender<HashMap<String, ServiceSnapshot>>,
        >,
    ) -> Result<Self, IpcError> {
        Self::with_log_store(socket_path, command_tx, snapshot_req_tx, None)
    }

    pub fn with_log_store(
        socket_path: PathBuf,
        command_tx: mpsc::UnboundedSender<CommandRequest>,
        snapshot_req_tx: mpsc::UnboundedSender<
            mpsc::UnboundedSender<HashMap<String, ServiceSnapshot>>,
        >,
        log_store: Option<Arc<LogStore>>,
    ) -> Result<Self, IpcError> {
        Self::with_heartbeat_tx(socket_path, command_tx, snapshot_req_tx, None, log_store)
    }

    pub fn with_heartbeat_tx(
        socket_path: PathBuf,
        command_tx: mpsc::UnboundedSender<CommandRequest>,
        snapshot_req_tx: mpsc::UnboundedSender<
            mpsc::UnboundedSender<HashMap<String, ServiceSnapshot>>,
        >,
        heartbeat_tx: Option<mpsc::UnboundedSender<HeartbeatMessage>>,
        log_store: Option<Arc<LogStore>>,
    ) -> Result<Self, IpcError> {
        // Remove existing socket if it exists
        if socket_path.exists() {
            std::fs::remove_file(&socket_path)?;
        }

        let (event_broadcast, _) = broadcast::channel(100);

        Ok(Self {
            socket_path,
            event_broadcast,
            command_tx,
            snapshot_req_tx,
            heartbeat_tx,
            log_store,
            shutdown: Arc::new(Mutex::new(false)),
        })
    }

    pub async fn start(&self) -> Result<(), IpcError> {
        info!("Starting IPC server on {:?}", self.socket_path);

        let listener = UnixListener::bind(&self.socket_path)?;

        // Set permissions to 0600 (owner read/write only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = std::fs::metadata(&self.socket_path)?;
            let mut permissions = metadata.permissions();
            permissions.set_mode(0o600);
            std::fs::set_permissions(&self.socket_path, permissions)?;
        }

        info!("IPC server listening on {:?}", self.socket_path);

        loop {
            if *self.shutdown.lock().await {
                break;
            }

            match listener.accept().await {
                Ok((stream, _addr)) => {
                    debug!("New client connected");
                    let (handler, writer) = ClientHandler::new(
                        stream,
                        self.event_broadcast.clone(),
                        self.command_tx.clone(),
                        self.snapshot_req_tx.clone(),
                        self.heartbeat_tx.clone(),
                        self.log_store.clone(),
                    );

                    tokio::spawn(async move {
                        if let Err(e) = handler.handle(writer).await {
                            error!("Client handler error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                }
            }
        }

        info!("IPC server stopped");
        Ok(())
    }

    pub async fn shutdown(&self) {
        *self.shutdown.lock().await = true;

        // Remove socket file
        if self.socket_path.exists() {
            if let Err(e) = std::fs::remove_file(&self.socket_path) {
                error!("Failed to remove socket file: {}", e);
            }
        }
    }

    /// Broadcast an event to all connected clients
    pub fn broadcast_event(&self, service: String, status: ServiceStatus) {
        let message = ServerMessage::StatusUpdate { service, status };
        let _ = self.event_broadcast.send(message);
    }

    /// Broadcast a log message to clients
    pub fn broadcast_log(&self, service: String, line: String) {
        let message = ServerMessage::LogLine { service, line };
        let _ = self.event_broadcast.send(message);
    }
}

struct ClientHandler {
    event_rx: broadcast::Receiver<ServerMessage>,
    command_tx: mpsc::UnboundedSender<CommandRequest>,
    snapshot_req_tx: mpsc::UnboundedSender<mpsc::UnboundedSender<HashMap<String, ServiceSnapshot>>>,
    heartbeat_tx: Option<mpsc::UnboundedSender<HeartbeatMessage>>,
    log_store: Option<Arc<LogStore>>,
    reader: BufReader<tokio::io::ReadHalf<UnixStream>>,
}

impl ClientHandler {
    fn new(
        stream: UnixStream,
        event_broadcast: broadcast::Sender<ServerMessage>,
        command_tx: mpsc::UnboundedSender<CommandRequest>,
        snapshot_req_tx: mpsc::UnboundedSender<
            mpsc::UnboundedSender<HashMap<String, ServiceSnapshot>>,
        >,
        heartbeat_tx: Option<mpsc::UnboundedSender<HeartbeatMessage>>,
        log_store: Option<Arc<LogStore>>,
    ) -> (Self, tokio::io::WriteHalf<UnixStream>) {
        let event_rx = event_broadcast.subscribe();
        let (reader, writer) = tokio::io::split(stream);
        let reader = BufReader::new(reader);

        let handler = Self {
            event_rx,
            command_tx,
            snapshot_req_tx,
            heartbeat_tx,
            log_store,
            reader,
        };

        (handler, writer)
    }

    async fn handle(
        mut self,
        mut writer: tokio::io::WriteHalf<UnixStream>,
    ) -> Result<(), IpcError> {
        let mut line_buffer = String::new();

        // Create response channel for sending messages back to client
        let (response_tx, mut response_rx) = mpsc::unbounded_channel::<ServerMessage>();

        // Spawn task to forward events and responses to this client
        let mut event_rx = self.event_rx.resubscribe();
        let (close_tx, mut close_rx) = mpsc::channel::<()>(1);

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    event = event_rx.recv() => {
                        match event {
                            Ok(message) => {
                                if let Ok(json) = serde_json::to_string(&message) {
                                    let line = format!("{}\n", json);
                                    if writer.write_all(line.as_bytes()).await.is_err() {
                                        break;
                                    }
                                }
                            }
                            Err(broadcast::error::RecvError::Lagged(_)) => {
                                warn!("Client lagging behind on events");
                            }
                            Err(_) => break,
                        }
                    }
                    response = response_rx.recv() => {
                        if let Some(message) = response {
                            if let Ok(json) = serde_json::to_string(&message) {
                                let line = format!("{}\n", json);
                                if writer.write_all(line.as_bytes()).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                    _ = close_rx.recv() => {
                        break;
                    }
                }
            }
        });

        // Read client messages
        loop {
            line_buffer.clear();

            match self.reader.read_line(&mut line_buffer).await {
                Ok(0) => {
                    debug!("Client disconnected");
                    break;
                }
                Ok(_) => {
                    let trimmed = line_buffer.trim();
                    if trimmed.is_empty() {
                        continue;
                    }

                    match serde_json::from_str::<ClientMessage>(trimmed) {
                        Ok(message) => {
                            if let Err(e) = self.handle_message(message, &response_tx).await {
                                error!("Error handling message: {}", e);
                            }
                        }
                        Err(e) => {
                            error!("Failed to parse client message: {}", e);
                            // Error responses would need a separate writer channel
                            // For now, just log the error
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to read from client: {}", e);
                    break;
                }
            }
        }

        let _ = close_tx.send(()).await;
        Ok(())
    }

    async fn handle_message(
        &mut self,
        message: ClientMessage,
        response_tx: &mpsc::UnboundedSender<ServerMessage>,
    ) -> Result<(), IpcError> {
        match message {
            ClientMessage::Heartbeat {
                service,
                status,
                metadata,
            } => {
                debug!("Received heartbeat from service '{}'", service);
                if let Some(ref tx) = self.heartbeat_tx {
                    let _ = tx.send((service, status, metadata));
                }
            }

            ClientMessage::Command { action, target } => {
                debug!("Received command: {:?} for {:?}", action, target);
                self.command_tx
                    .send((action, target))
                    .map_err(|_| IpcError::InvalidMessage("Failed to send command".to_string()))?;

                // Send acknowledgment
                let ack = ServerMessage::Ack { request_id: None };
                let _ = response_tx.send(ack);
            }

            ClientMessage::Subscribe { events, logs } => {
                debug!("Client subscribed - events: {}, logs: {:?}", events, logs);
                // Subscription is handled automatically via broadcast channel
            }

            ClientMessage::GetSnapshot => {
                debug!("Client requested snapshot");

                // Create a channel to receive the snapshot
                let (snapshot_tx, mut snapshot_rx) = mpsc::unbounded_channel();

                // Send request to orchestrator
                if self.snapshot_req_tx.send(snapshot_tx).is_err() {
                    error!("Failed to request snapshot from orchestrator");
                    return Ok(());
                }

                // Wait for response (with timeout)
                tokio::select! {
                    snapshot = snapshot_rx.recv() => {
                        if let Some(services) = snapshot {
                            let response = ServerMessage::Snapshot { services };
                            let _ = response_tx.send(response);
                        }
                    }
                    _ = tokio::time::sleep(tokio::time::Duration::from_secs(1)) => {
                        error!("Timeout waiting for snapshot");
                    }
                }
            }

            ClientMessage::GetLogs { service } => {
                debug!("Client requested logs for: {:?}", service);

                let lines = if let Some(ref log_store) = self.log_store {
                    log_store.get_logs(service.as_deref(), 1000).await
                } else {
                    vec!["Log store not available.".to_string()]
                };

                let response = ServerMessage::LogHistory { service, lines };
                let _ = response_tx.send(response);
            }
        }

        Ok(())
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        if self.socket_path.exists() {
            let _ = std::fs::remove_file(&self.socket_path);
        }
    }
}

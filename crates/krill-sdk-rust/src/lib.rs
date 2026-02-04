// Krill Rust SDK - Client library for sending heartbeats to krill daemon

use krill_common::{ClientMessage, ServiceStatus};
use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tokio::sync::Mutex;

pub struct KrillClient {
    service_name: String,
    stream: Mutex<UnixStream>,
}

impl KrillClient {
    /// Create a new Krill client
    pub async fn new(service_name: &str) -> Result<Self, KrillError> {
        Self::connect(service_name, PathBuf::from("/tmp/krill.sock")).await
    }

    /// Create a new Krill client with custom socket path
    pub async fn connect(service_name: &str, socket_path: PathBuf) -> Result<Self, KrillError> {
        let stream = UnixStream::connect(&socket_path)
            .await
            .map_err(|e| KrillError::Connection(e.to_string()))?;

        Ok(Self {
            service_name: service_name.to_string(),
            stream: Mutex::new(stream),
        })
    }

    /// Send a heartbeat to the daemon
    pub async fn heartbeat(&self) -> Result<(), KrillError> {
        self.send_heartbeat(ServiceStatus::Healthy, HashMap::new())
            .await
    }

    /// Send a heartbeat with custom metadata
    pub async fn heartbeat_with_metadata(
        &self,
        metadata: HashMap<String, String>,
    ) -> Result<(), KrillError> {
        self.send_heartbeat(ServiceStatus::Healthy, metadata).await
    }

    /// Report degraded status
    pub async fn report_degraded(&self, reason: &str) -> Result<(), KrillError> {
        let mut metadata = HashMap::new();
        metadata.insert("reason".to_string(), reason.to_string());
        self.send_heartbeat(ServiceStatus::Degraded, metadata).await
    }

    /// Report healthy status
    pub async fn report_healthy(&self) -> Result<(), KrillError> {
        self.send_heartbeat(ServiceStatus::Healthy, HashMap::new())
            .await
    }

    async fn send_heartbeat(
        &self,
        status: ServiceStatus,
        metadata: HashMap<String, String>,
    ) -> Result<(), KrillError> {
        let message = ClientMessage::Heartbeat {
            service: self.service_name.clone(),
            status,
            metadata,
        };

        let json = serde_json::to_string(&message)
            .map_err(|e| KrillError::Serialization(e.to_string()))?;

        let mut stream = self.stream.lock().await;
        stream
            .write_all(format!("{}\n", json).as_bytes())
            .await
            .map_err(|e| KrillError::Io(e))?;

        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum KrillError {
    #[error("Connection error: {0}")]
    Connection(String),

    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Serialization error: {0}")]
    Serialization(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        // Can't test actual connection without daemon running
        // Just verify the API compiles
        let _client_future = KrillClient::new("test-service");
    }

    #[test]
    fn test_heartbeat_message_format() {
        let message = ClientMessage::Heartbeat {
            service: "test".to_string(),
            status: ServiceStatus::Healthy,
            metadata: HashMap::new(),
        };

        let json = serde_json::to_string(&message).unwrap();
        assert!(json.contains("test"));
        assert!(json.contains("healthy"));
    }
}

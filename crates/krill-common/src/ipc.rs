// Phase 1.5: IPC Protocol

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Heartbeat {
        service: String,
        status: ServiceStatus,
        #[serde(default)]
        metadata: HashMap<String, String>,
    },
    Command {
        action: CommandAction,
        #[serde(default)]
        target: Option<String>,
    },
    Subscribe {
        events: bool,
        logs: Option<String>,
    },
    GetSnapshot,
    GetLogs {
        service: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandAction {
    Start,
    Stop,
    Restart,
    Kill,
    StopDaemon,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceStatus {
    Starting,
    Running,
    Healthy,
    Degraded,
    Stopping,
    Stopped,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Ack {
        request_id: Option<String>,
    },
    Error {
        message: String,
        code: Option<i32>,
    },
    StatusUpdate {
        service: String,
        status: ServiceStatus,
    },
    LogLine {
        service: String,
        line: String,
    },
    Snapshot {
        services: HashMap<String, ServiceSnapshot>,
    },
    LogHistory {
        service: Option<String>,
        lines: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServiceSnapshot {
    pub status: ServiceStatus,
    pub pid: Option<u32>,
    pub uptime: Option<std::time::Duration>,
    pub restart_count: u32,
    pub last_error: Option<String>,
    pub namespace: String,
    pub executor_type: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heartbeat_message() {
        let mut metadata = HashMap::new();
        metadata.insert("version".to_string(), "1.0".to_string());

        let msg = ClientMessage::Heartbeat {
            service: "lidar".to_string(),
            status: ServiceStatus::Healthy,
            metadata,
        };

        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: ClientMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, deserialized);
    }

    #[test]
    fn test_command_message() {
        let msg = ClientMessage::Command {
            action: CommandAction::Start,
            target: Some("navigator".to_string()),
        };

        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"command\""));
        assert!(json.contains("\"action\":\"start\""));
    }

    #[test]
    fn test_server_error() {
        let msg = ServerMessage::Error {
            message: "Service not found".to_string(),
            code: Some(404),
        };

        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: ServerMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, deserialized);
    }

    #[test]
    fn test_snapshot_message() {
        let mut services = HashMap::new();
        services.insert(
            "lidar".to_string(),
            ServiceSnapshot {
                status: ServiceStatus::Running,
                pid: Some(1234),
                uptime: Some(std::time::Duration::from_secs(300)),
                restart_count: 0,
                last_error: None,
                namespace: "test-workspace".to_string(),
                executor_type: "pixi".to_string(),
            },
        );

        let msg = ServerMessage::Snapshot { services };
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: ServerMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, deserialized);
    }
}

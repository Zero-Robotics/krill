use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// ============================================================================
// Service Configuration Models
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RestartPolicyCondition {
    Always,
    Never,
    OnFailure,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RestartPolicy {
    pub condition: RestartPolicyCondition,
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u32,
    #[serde(default = "default_delay_sec")]
    pub delay_sec: u32,
}

fn default_max_attempts() -> u32 {
    3
}

fn default_delay_sec() -> u32 {
    2
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum HealthCheckType {
    Heartbeat,
    Tcp,
    Command,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HealthCheck {
    #[serde(rename = "type")]
    pub check_type: HealthCheckType,
    #[serde(default = "default_timeout_sec")]
    pub timeout_sec: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

fn default_timeout_sec() -> u32 {
    5
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DependencyCondition {
    Started,
    Healthy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Dependency {
    pub service: String,
    pub condition: DependencyCondition,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServiceConfig {
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_cmd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restart_policy: Option<RestartPolicy>,
    #[serde(default)]
    pub critical: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check: Option<HealthCheck>,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_directory: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServicesConfig {
    pub version: String,
    pub services: HashMap<String, ServiceConfig>,
}

// ============================================================================
// Service State Models
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ServiceState {
    Starting,
    Running,
    Healthy,
    Stopping,
    Stopped,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceStatus {
    pub service: String,
    pub state: ServiceState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_heartbeat: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restart_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

// ============================================================================
// IPC Message Models
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "type")]
pub enum Message {
    Heartbeat(HeartbeatMessage),
    Request(RequestMessage),
    Response(ResponseMessage),
    Event(EventMessage),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatMessage {
    #[serde(default = "default_message_version")]
    pub version: String,
    pub service: String,
    pub status: ServiceState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    #[serde(default = "Utc::now")]
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestMessage {
    #[serde(default = "default_message_version")]
    pub version: String,
    #[serde(default = "Uuid::new_v4")]
    pub id: Uuid,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseMessage {
    #[serde(default = "default_message_version")]
    pub version: String,
    pub id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ResponseError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    StateTransition,
    ServiceStarted,
    ServiceStopped,
    ServiceFailed,
    CriticalFailure,
    EmergencyStop,
    LogMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventMessage {
    #[serde(default = "default_message_version")]
    pub version: String,
    pub event: EventType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<ServiceState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<ServiceState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    #[serde(default = "Utc::now")]
    pub timestamp: DateTime<Utc>,
}

fn default_message_version() -> String {
    "1.0".to_string()
}

// ============================================================================
// Helper Functions
// ============================================================================

impl ServiceStatus {
    pub fn new(service: String, state: ServiceState) -> Self {
        Self {
            service,
            state,
            pid: None,
            exit_code: None,
            last_heartbeat: None,
            restart_count: None,
            metadata: None,
        }
    }
}

impl HeartbeatMessage {
    pub fn new(service: String, status: ServiceState) -> Self {
        Self {
            version: default_message_version(),
            service,
            status,
            metadata: None,
            timestamp: Utc::now(),
        }
    }
}

impl RequestMessage {
    pub fn new(method: String, params: Option<serde_json::Value>) -> Self {
        Self {
            version: default_message_version(),
            id: Uuid::new_v4(),
            method,
            params,
        }
    }
}

impl ResponseMessage {
    pub fn success(id: Uuid, result: Option<serde_json::Value>) -> Self {
        Self {
            version: default_message_version(),
            id,
            result,
            error: None,
        }
    }

    pub fn error(id: Uuid, code: i32, message: String, data: Option<serde_json::Value>) -> Self {
        Self {
            version: default_message_version(),
            id,
            result: None,
            error: Some(ResponseError {
                code,
                message,
                data,
            }),
        }
    }
}

impl EventMessage {
    pub fn state_transition(service: String, from: ServiceState, to: ServiceState) -> Self {
        Self {
            version: default_message_version(),
            event: EventType::StateTransition,
            service: Some(service),
            from: Some(from),
            to: Some(to),
            pid: None,
            exit_code: None,
            metadata: None,
            timestamp: Utc::now(),
        }
    }

    pub fn critical_failure(service: String, exit_code: Option<i32>) -> Self {
        Self {
            version: default_message_version(),
            event: EventType::CriticalFailure,
            service: Some(service),
            from: None,
            to: None,
            pid: None,
            exit_code,
            metadata: None,
            timestamp: Utc::now(),
        }
    }
}

// ============================================================================
// Validation
// ============================================================================

impl ServiceConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.command.trim().is_empty() {
            return Err("Service command cannot be empty".to_string());
        }

        if let Some(ref health_check) = self.health_check {
            match health_check.check_type {
                HealthCheckType::Tcp => {
                    if health_check.port.is_none() {
                        return Err("TCP health check requires a port".to_string());
                    }
                }
                HealthCheckType::Command => {
                    if health_check.command.is_none()
                        || health_check.command.as_ref().unwrap().trim().is_empty()
                    {
                        return Err("Command health check requires a command".to_string());
                    }
                }
                _ => {}
            }

            if health_check.timeout_sec == 0 {
                return Err("Health check timeout must be positive".to_string());
            }
        }

        Ok(())
    }
}

impl ServicesConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.services.is_empty() {
            return Err("No services defined".to_string());
        }

        // Check for circular dependencies
        let mut visited = std::collections::HashSet::new();
        let mut stack = std::collections::HashSet::new();

        for service_name in self.services.keys() {
            if !visited.contains(service_name) {
                if self.has_circular_dependency(service_name, &mut visited, &mut stack) {
                    return Err(format!(
                        "Circular dependency detected involving service '{}'",
                        service_name
                    ));
                }
            }
        }

        // Validate each service
        for (name, config) in &self.services {
            if let Err(e) = config.validate() {
                return Err(format!("Service '{}': {}", name, e));
            }

            // Validate dependencies exist
            for dep in &config.dependencies {
                if !self.services.contains_key(&dep.service) {
                    return Err(format!(
                        "Service '{}' depends on unknown service '{}'",
                        name, dep.service
                    ));
                }
            }
        }

        Ok(())
    }

    fn has_circular_dependency(
        &self,
        service: &str,
        visited: &mut std::collections::HashSet<String>,
        stack: &mut std::collections::HashSet<String>,
    ) -> bool {
        if stack.contains(service) {
            return true;
        }

        if visited.contains(service) {
            return false;
        }

        visited.insert(service.to_string());
        stack.insert(service.to_string());

        if let Some(config) = self.services.get(service) {
            for dep in &config.dependencies {
                if self.has_circular_dependency(&dep.service, visited, stack) {
                    return true;
                }
            }
        }

        stack.remove(service);
        false
    }
}

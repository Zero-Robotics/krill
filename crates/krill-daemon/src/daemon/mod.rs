use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use krill_common::model::{ServiceState, ServiceStatus, ServicesConfig};
use tokio::sync::{Mutex, RwLock, mpsc};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::dag::DependencyGraph;
use crate::health::HealthMonitor;
use crate::process::ProcessManager;
use crate::safety::SafetyInterceptor;

/// Main daemon structure holding all system state
pub struct Daemon {
    /// Configuration loaded from YAML
    config: ServicesConfig,
    /// Current state of all services
    service_states: Arc<RwLock<HashMap<String, ServiceStatus>>>,
    /// Dependency graph for service orchestration
    dependency_graph: DependencyGraph,
    /// Process manager for spawning/killing processes
    process_manager: ProcessManager,
    /// Health monitor for tracking service health
    health_monitor: HealthMonitor,
    /// Safety interceptor for critical failures
    safety_interceptor: SafetyInterceptor,
    /// PID file path
    pid_file: PathBuf,
    /// IPC socket path
    socket_path: PathBuf,
    /// Log directory for service outputs
    log_dir: PathBuf,
    /// Command channel for supervisor commands
    command_tx: mpsc::Sender<DaemonCommand>,
    /// Command receiver for supervisor commands
    command_rx: Arc<Mutex<Option<mpsc::Receiver<DaemonCommand>>>>,
    /// Event channel for broadcasting system events
    event_tx: mpsc::Sender<DaemonEvent>,
    /// Event receiver for broadcasting system events
    event_rx: Arc<Mutex<Option<mpsc::Receiver<DaemonEvent>>>>,
    /// Daemon session ID for tracking
    session_id: Uuid,
    /// Emergency mode flag
    emergency_mode: Arc<Mutex<bool>>,
}

/// Commands that can be sent to the daemon
#[derive(Debug)]
pub enum DaemonCommand {
    StartService(String),
    StopService(String),
    RestartService(String),
    EmergencyStop,
    GetStatus,
    GetServiceStatus(String),
    GetServiceLogs(String, usize), // service name, number of lines
    Shutdown,
}

/// Events emitted by the daemon
#[derive(Debug, Clone)]
pub enum DaemonEvent {
    ServiceStateChanged {
        service: String,
        old_state: ServiceState,
        new_state: ServiceState,
        pid: Option<u32>,
    },
    ServiceStarted(String, u32),                // service name, pid
    ServiceStopped(String, Option<i32>),        // service name, exit code
    ServiceFailed(String, Option<i32>, String), // service name, exit code, reason
    CriticalFailure(String, String),            // service name, reason
    EmergencyStopTriggered(String),             // reason
    HeartbeatReceived(String),                  // service name
    DependencySatisfied(String, String),        // dependent, dependency
    LogMessage {
        service: Option<String>,
        level: tracing::Level,
        message: String,
    },
}

impl Daemon {
    /// Create a new daemon instance
    pub async fn new(
        config: ServicesConfig,
        pid_file: PathBuf,
        socket_path: PathBuf,
        log_dir: PathBuf,
    ) -> Result<Self, String> {
        info!(
            "Initializing Krill daemon with {} services",
            config.services.len()
        );

        // Create directories
        if let Err(e) = std::fs::create_dir_all(&log_dir) {
            warn!(
                "Failed to create log directory {}: {}",
                log_dir.display(),
                e
            );
        }

        // Expand home directory if needed
        let log_dir = expand_home_dir(&log_dir);

        // Initialize service states
        let mut service_states = HashMap::new();
        for (service_name, service_config) in &config.services {
            service_states.insert(
                service_name.clone(),
                ServiceStatus::new(service_name.clone(), ServiceState::Stopped),
            );
        }

        // Create dependency graph
        let dependency_graph = DependencyGraph::new(&config)
            .map_err(|e| format!("Failed to create dependency graph: {}", e))?;

        // Create channels
        let (command_tx, command_rx) = mpsc::channel(100);
        let (event_tx, event_rx) = mpsc::channel(100);

        // Create process manager
        let process_manager = ProcessManager::new(log_dir.clone(), event_tx.clone())
            .map_err(|e| format!("Failed to create process manager: {}", e))?;

        // Create health monitor
        let health_monitor = HealthMonitor::new(config.clone(), event_tx.clone())
            .map_err(|e| format!("Failed to create health monitor: {}", e))?;

        // Create safety interceptor
        let safety_interceptor = SafetyInterceptor::new(
            dependency_graph.clone(),
            process_manager.clone(),
            event_tx.clone(),
        );

        let daemon = Daemon {
            config,
            service_states: Arc::new(RwLock::new(service_states)),
            dependency_graph,
            process_manager,
            health_monitor,
            safety_interceptor,
            pid_file,
            socket_path,
            log_dir,
            command_tx,
            command_rx: Arc::new(Mutex::new(Some(command_rx))),
            event_tx,
            event_rx: Arc::new(Mutex::new(Some(event_rx))),
            session_id: Uuid::new_v4(),
            emergency_mode: Arc::new(Mutex::new(false)),
        };

        // Write PID file
        if let Err(e) = daemon.write_pid_file().await {
            warn!("Failed to write PID file: {}", e);
        }

        info!("Daemon initialized with session ID: {}", daemon.session_id);
        Ok(daemon)
    }

    /// Write the current process ID to the PID file
    async fn write_pid_file(&self) -> Result<(), String> {
        let pid = std::process::id();
        let content = pid.to_string();

        if let Some(parent) = self.pid_file.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create PID file directory: {}", e))?;
        }

        std::fs::write(&self.pid_file, &content)
            .map_err(|e| format!("Failed to write PID file: {}", e))?;

        info!("PID {} written to {}", pid, self.pid_file.display());
        Ok(())
    }

    /// Remove the PID file
    async fn remove_pid_file(&self) -> Result<(), String> {
        if self.pid_file.exists() {
            std::fs::remove_file(&self.pid_file)
                .map_err(|e| format!("Failed to remove PID file: {}", e))?;
            info!("PID file removed: {}", self.pid_file.display());
        }
        Ok(())
    }

    /// Get the command sender for sending commands to the daemon
    pub fn command_sender(&self) -> mpsc::Sender<DaemonCommand> {
        self.command_tx.clone()
    }

    /// Take the command receiver for supervisor commands
    /// This can only be called once, as it takes ownership of the receiver
    pub async fn take_command_receiver(&self) -> Result<mpsc::Receiver<DaemonCommand>, String> {
        let mut rx_opt = self.command_rx.lock().await;
        rx_opt
            .take()
            .ok_or("Command receiver already taken".to_string())
    }

    /// Take the event receiver for system events
    /// This can only be called once, as it takes ownership of the receiver
    pub async fn take_event_receiver(&self) -> Result<mpsc::Receiver<DaemonEvent>, String> {
        let mut rx_opt = self.event_rx.lock().await;
        rx_opt
            .take()
            .ok_or("Event receiver already taken".to_string())
    }

    /// Get the event sender for emitting daemon events
    pub fn event_sender(&self) -> mpsc::Sender<DaemonEvent> {
        self.event_tx.clone()
    }

    /// Get the current configuration
    pub fn config(&self) -> &ServicesConfig {
        &self.config
    }

    /// Get the socket path for IPC
    pub fn socket_path(&self) -> &PathBuf {
        &self.socket_path
    }

    /// Get the log directory
    pub fn log_dir(&self) -> &PathBuf {
        &self.log_dir
    }

    /// Get the session ID
    pub fn session_id(&self) -> Uuid {
        self.session_id
    }

    /// Check if daemon is in emergency mode
    pub async fn is_emergency_mode(&self) -> bool {
        *self.emergency_mode.lock().await
    }

    /// Set emergency mode
    pub async fn set_emergency_mode(&self, enabled: bool) {
        let mut mode = self.emergency_mode.lock().await;
        *mode = enabled;

        if enabled {
            warn!("EMERGENCY MODE ACTIVATED");
        } else {
            info!("Emergency mode deactivated");
        }
    }

    /// Get current service states
    pub async fn get_service_states(&self) -> HashMap<String, ServiceStatus> {
        self.service_states.read().await.clone()
    }

    /// Update a service state
    pub async fn update_service_state(
        &self,
        service: &str,
        state: ServiceState,
        pid: Option<u32>,
        exit_code: Option<i32>,
    ) -> Result<(), String> {
        let mut states = self.service_states.write().await;

        if let Some(status) = states.get_mut(service) {
            let old_state = status.state.clone();
            status.state = state.clone();
            status.pid = pid;
            status.exit_code = exit_code;

            // Update heartbeat timestamp if transitioning to healthy
            if matches!(status.state, ServiceState::Healthy) {
                status.last_heartbeat = Some(chrono::Utc::now());
            }

            info!(
                "Service '{}' state changed: {:?} -> {:?}",
                service, old_state, state
            );

            // Emit event
            let _ = self
                .event_tx
                .send(DaemonEvent::ServiceStateChanged {
                    service: service.to_string(),
                    old_state,
                    new_state: status.state.clone(),
                    pid,
                })
                .await;
        } else {
            return Err(format!("Service '{}' not found", service));
        }

        Ok(())
    }

    /// Get a service configuration
    pub fn get_service_config(&self, service: &str) -> Option<&krill_common::model::ServiceConfig> {
        self.config.services.get(service)
    }

    /// Get dependency graph
    pub fn dependency_graph(&self) -> &DependencyGraph {
        &self.dependency_graph
    }

    /// Get process manager
    pub fn process_manager(&self) -> &ProcessManager {
        &self.process_manager
    }

    /// Get health monitor
    pub fn health_monitor(&self) -> &HealthMonitor {
        &self.health_monitor
    }

    /// Get safety interceptor
    pub fn safety_interceptor(&self) -> &SafetyInterceptor {
        &self.safety_interceptor
    }

    /// Gracefully shutdown the daemon
    pub async fn shutdown(self) -> Result<(), String> {
        info!("Shutting down daemon...");

        // Stop all services
        self.process_manager
            .stop_all()
            .await
            .map_err(|e| format!("Failed to stop all services: {}", e))?;

        // Remove PID file
        self.remove_pid_file().await?;

        info!("Daemon shutdown complete");
        Ok(())
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        // Clean up PID file on drop (e.g., on panic or early exit)
        if self.pid_file.exists() {
            if let Err(e) = std::fs::remove_file(&self.pid_file) {
                eprintln!("Failed to remove PID file on drop: {}", e);
            }
        }
    }
}

/// Expand home directory in paths starting with ~
fn expand_home_dir(path: &PathBuf) -> PathBuf {
    let path_str = path.to_string_lossy();
    if path_str.starts_with('~') {
        if let Some(home) = dirs::home_dir() {
            let expanded = home.join(&path_str[1..].trim_start_matches('/'));
            return expanded;
        }
    }
    path.clone()
}

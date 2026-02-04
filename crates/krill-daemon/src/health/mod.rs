use std::collections::HashMap;
use std::net::TcpStream;
use std::sync::Arc;
use std::time::{Duration, Instant};

use krill_common::model::{HealthCheck, HealthCheckType, ServiceState, ServicesConfig};
use tokio::process::Command;
use tokio::sync::{Mutex, RwLock, mpsc};
use tokio::time;
use tracing::{debug, error, info, warn};

use crate::daemon::DaemonEvent;

/// Health monitor for tracking service health via heartbeats and health checks
pub struct HealthMonitor {
    /// Service configuration
    config: ServicesConfig,
    /// Last heartbeat timestamps per service
    heartbeats: Arc<RwLock<HashMap<String, Instant>>>,
    /// Last health check results per service
    health_status: Arc<RwLock<HashMap<String, bool>>>,
    /// Event sender for health status changes
    event_tx: mpsc::Sender<DaemonEvent>,
    /// Background task handles
    tasks: Arc<Mutex<HashMap<String, tokio::task::JoinHandle<()>>>>,
    /// Shutdown signal
    shutdown: Arc<Mutex<bool>>,
}

impl HealthMonitor {
    /// Create a new health monitor
    pub fn new(
        config: ServicesConfig,
        event_tx: mpsc::Sender<DaemonEvent>,
    ) -> Result<Self, String> {
        let mut heartbeats = HashMap::new();
        let mut health_status = HashMap::new();

        // Initialize tracking for all services
        for service_name in config.services.keys() {
            heartbeats.insert(service_name.clone(), Instant::now());
            health_status.insert(service_name.clone(), false); // Start as unhealthy
        }

        Ok(Self {
            config,
            heartbeats: Arc::new(RwLock::new(heartbeats)),
            health_status: Arc::new(RwLock::new(health_status)),
            event_tx,
            tasks: Arc::new(Mutex::new(HashMap::new())),
            shutdown: Arc::new(Mutex::new(false)),
        })
    }

    /// Start health monitoring for all services
    pub async fn start(&self) -> Result<(), String> {
        info!("Starting health monitoring for all services");

        for (service_name, service_config) in &self.config.services {
            if let Some(health_check) = &service_config.health_check {
                self.start_service_monitoring(service_name.clone(), health_check.clone())
                    .await?;
            }
        }

        Ok(())
    }

    /// Start monitoring for a specific service
    async fn start_service_monitoring(
        &self,
        service_name: String,
        health_check: HealthCheck,
    ) -> Result<(), String> {
        let heartbeats = self.heartbeats.clone();
        let health_status = self.health_status.clone();
        let event_tx = self.event_tx.clone();
        let shutdown = self.shutdown.clone();

        // Calculate check interval based on timeout (check 3x more frequently than timeout)
        let check_interval = Duration::from_secs(health_check.timeout_sec as u64 / 3);
        let timeout_duration = Duration::from_secs(health_check.timeout_sec as u64);

        let service_name_for_task = service_name.clone();
        let service_name_for_map = service_name.clone();
        let handle = tokio::spawn(async move {
            let mut interval = time::interval(check_interval.max(Duration::from_millis(100)));

            loop {
                interval.tick().await;

                // Check for shutdown
                if *shutdown.lock().await {
                    break;
                }

                match health_check.check_type {
                    HealthCheckType::Heartbeat => {
                        // Check heartbeat timeout
                        let last_heartbeat = {
                            let heartbeats = heartbeats.read().await;
                            heartbeats.get(&service_name_for_task).copied()
                        };

                        if let Some(last_heartbeat) = last_heartbeat {
                            let elapsed = last_heartbeat.elapsed();
                            let is_healthy = elapsed < timeout_duration;

                            // Update health status if changed
                            let mut health_status = health_status.write().await;
                            let old_status =
                                health_status.insert(service_name_for_task.clone(), is_healthy);

                            if old_status != Some(is_healthy) {
                                let event = if is_healthy {
                                    DaemonEvent::ServiceStateChanged {
                                        service: service_name_for_task.clone(),
                                        old_state: ServiceState::Running,
                                        new_state: ServiceState::Healthy,
                                        pid: None,
                                    }
                                } else {
                                    DaemonEvent::ServiceFailed(
                                        service_name_for_task.clone(),
                                        None,
                                        format!(
                                            "Heartbeat timeout: {}s elapsed",
                                            elapsed.as_secs()
                                        ),
                                    )
                                };

                                let _ = event_tx.send(event).await;
                            }

                            if !is_healthy {
                                warn!(
                                    "Service '{}' heartbeat timeout: {}s > {}s",
                                    service_name_for_task,
                                    elapsed.as_secs(),
                                    timeout_duration.as_secs()
                                );
                            } else {
                                debug!(
                                    "Service '{}' heartbeat OK: {}s < {}s",
                                    service_name_for_task,
                                    elapsed.as_secs(),
                                    timeout_duration.as_secs()
                                );
                            }
                        }
                    }
                    HealthCheckType::Tcp => {
                        if let Some(port) = health_check.port {
                            match Self::perform_tcp_check(port).await {
                                Ok(true) => {
                                    HealthMonitor::update_health_status(
                                        &service_name_for_task,
                                        true,
                                        &health_status,
                                        &event_tx,
                                    )
                                    .await;
                                }
                                Ok(false) => {
                                    HealthMonitor::update_health_status(
                                        &service_name_for_task,
                                        false,
                                        &health_status,
                                        &event_tx,
                                    )
                                    .await;
                                    warn!("Service '{}' TCP check failed", service_name_for_task);
                                }
                                Err(e) => {
                                    error!(
                                        "TCP check error for service '{}': {}",
                                        service_name_for_task, e
                                    );
                                    HealthMonitor::update_health_status(
                                        &service_name_for_task,
                                        false,
                                        &health_status,
                                        &event_tx,
                                    )
                                    .await;
                                }
                            }
                        }
                    }
                    HealthCheckType::Command => {
                        if let Some(command) = &health_check.command {
                            match Self::perform_command_check(command).await {
                                Ok(true) => {
                                    HealthMonitor::update_health_status(
                                        &service_name_for_task,
                                        true,
                                        &health_status,
                                        &event_tx,
                                    )
                                    .await;
                                }
                                Ok(false) => {
                                    HealthMonitor::update_health_status(
                                        &service_name_for_task,
                                        false,
                                        &health_status,
                                        &event_tx,
                                    )
                                    .await;
                                    warn!(
                                        "Service '{}' command check failed",
                                        service_name_for_task
                                    );
                                }
                                Err(e) => {
                                    error!(
                                        "Command check error for service '{}': {}",
                                        service_name_for_task, e
                                    );
                                    HealthMonitor::update_health_status(
                                        &service_name_for_task,
                                        false,
                                        &health_status,
                                        &event_tx,
                                    )
                                    .await;
                                }
                            }
                        }
                    }
                }
            }
        });

        let mut tasks = self.tasks.lock().await;
        tasks.insert(service_name_for_map, handle);

        Ok(())
    }

    /// Update health status and emit events if changed
    async fn update_health_status(
        service_name: &str,
        is_healthy: bool,
        health_status: &Arc<RwLock<HashMap<String, bool>>>,
        event_tx: &mpsc::Sender<DaemonEvent>,
    ) {
        let mut status = health_status.write().await;
        let old_status = status.insert(service_name.to_string(), is_healthy);

        if old_status != Some(is_healthy) {
            let event = if is_healthy {
                DaemonEvent::ServiceStateChanged {
                    service: service_name.to_string(),
                    old_state: ServiceState::Running,
                    new_state: ServiceState::Healthy,
                    pid: None,
                }
            } else {
                DaemonEvent::ServiceFailed(
                    service_name.to_string(),
                    None,
                    "Health check failed".to_string(),
                )
            };

            let _ = event_tx.send(event).await;
        }
    }

    /// Record a heartbeat for a service
    pub async fn record_heartbeat(&self, service_name: &str) -> Result<(), String> {
        debug!("Recording heartbeat for service '{}'", service_name);

        let mut heartbeats = self.heartbeats.write().await;
        if heartbeats.contains_key(service_name) {
            heartbeats.insert(service_name.to_string(), Instant::now());

            // Send heartbeat event
            let _ = self
                .event_tx
                .send(DaemonEvent::HeartbeatReceived(service_name.to_string()))
                .await;

            Ok(())
        } else {
            Err(format!("Service '{}' not found", service_name))
        }
    }

    /// Get the time since last heartbeat for a service
    pub async fn time_since_last_heartbeat(&self, service_name: &str) -> Option<Duration> {
        let heartbeats = self.heartbeats.read().await;
        heartbeats
            .get(service_name)
            .map(|instant| instant.elapsed())
    }

    /// Check if a service is currently healthy
    pub async fn is_service_healthy(&self, service_name: &str) -> bool {
        let health_status = self.health_status.read().await;
        health_status.get(service_name).copied().unwrap_or(false)
    }

    /// Get all unhealthy services
    pub async fn get_unhealthy_services(&self) -> Vec<String> {
        let health_status = self.health_status.read().await;
        health_status
            .iter()
            .filter(|(_, healthy)| !**healthy)
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// Get all healthy services
    pub async fn get_healthy_services(&self) -> Vec<String> {
        let health_status = self.health_status.read().await;
        health_status
            .iter()
            .filter(|(_, healthy)| **healthy)
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// Perform a TCP health check
    async fn perform_tcp_check(port: u16) -> Result<bool, String> {
        // Use async runtime for TCP connect
        let addr = format!("127.0.0.1:{}", port)
            .parse()
            .map_err(|e| format!("Invalid socket address: {}", e))?;

        let result = tokio::task::spawn_blocking(move || {
            TcpStream::connect_timeout(&addr, Duration::from_secs(2))
        })
        .await
        .map_err(|e| format!("TCP check task failed: {}", e))?;

        match result {
            Ok(_) => Ok(true),
            Err(e) => {
                debug!("TCP connection failed: {}", e);
                Ok(false)
            }
        }
    }

    /// Perform a command health check
    async fn perform_command_check(command: &str) -> Result<bool, String> {
        // Parse command using proper shell word splitting
        let parts = shell_words::split(command)
            .map_err(|e| format!("Failed to parse command '{}': {}", command, e))?;

        if parts.is_empty() {
            return Err("Empty command".to_string());
        }

        let executable = &parts[0];
        let args = &parts[1..];

        let output = Command::new(executable)
            .args(args)
            .output()
            .await
            .map_err(|e| format!("Failed to execute health check command: {}", e))?;

        // Command is considered healthy if it exits with code 0
        Ok(output.status.success())
    }

    /// Manually trigger a health check for a service
    pub async fn trigger_health_check(&self, service_name: &str) -> Result<bool, String> {
        if let Some(service_config) = self.config.services.get(service_name) {
            if let Some(health_check) = &service_config.health_check {
                match health_check.check_type {
                    HealthCheckType::Heartbeat => {
                        // For heartbeat, just check timeout
                        if let Some(elapsed) = self.time_since_last_heartbeat(service_name).await {
                            let timeout = Duration::from_secs(health_check.timeout_sec as u64);
                            Ok(elapsed < timeout)
                        } else {
                            Err(format!(
                                "No heartbeat recorded for service '{}'",
                                service_name
                            ))
                        }
                    }
                    HealthCheckType::Tcp => {
                        if let Some(port) = health_check.port {
                            Self::perform_tcp_check(port).await
                        } else {
                            Err("TCP health check requires a port".to_string())
                        }
                    }
                    HealthCheckType::Command => {
                        if let Some(command) = &health_check.command {
                            Self::perform_command_check(command).await
                        } else {
                            Err("Command health check requires a command".to_string())
                        }
                    }
                }
            } else {
                Err(format!(
                    "No health check configured for service '{}'",
                    service_name
                ))
            }
        } else {
            Err(format!("Service '{}' not found", service_name))
        }
    }

    /// Stop health monitoring
    pub async fn stop(&self) -> Result<(), String> {
        info!("Stopping health monitoring");

        // Set shutdown flag
        {
            let mut shutdown = self.shutdown.lock().await;
            *shutdown = true;
        }

        // Cancel all tasks
        let mut tasks = self.tasks.lock().await;
        for (service_name, handle) in tasks.drain() {
            handle.abort();
            debug!(
                "Cancelled health monitoring task for service '{}'",
                service_name
            );
        }

        Ok(())
    }

    /// Reset heartbeat for a service (useful after restart)
    pub async fn reset_heartbeat(&self, service_name: &str) -> Result<(), String> {
        let mut heartbeats = self.heartbeats.write().await;
        if heartbeats.contains_key(service_name) {
            heartbeats.insert(service_name.to_string(), Instant::now());
            debug!("Reset heartbeat for service '{}'", service_name);
            Ok(())
        } else {
            Err(format!("Service '{}' not found", service_name))
        }
    }

    /// Get health check configuration for a service
    pub fn get_health_check_config(&self, service_name: &str) -> Option<&HealthCheck> {
        self.config
            .services
            .get(service_name)
            .and_then(|config| config.health_check.as_ref())
    }
}

impl Clone for HealthMonitor {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            heartbeats: self.heartbeats.clone(),
            health_status: self.health_status.clone(),
            event_tx: self.event_tx.clone(),
            tasks: self.tasks.clone(), // Share task handles across clones
            shutdown: self.shutdown.clone(), // Share shutdown flag across clones
        }
    }
}

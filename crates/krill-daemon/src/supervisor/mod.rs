use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use krill_common::model::{RestartPolicyCondition, ServiceConfig, ServiceState, ServiceStatus};
use tokio::sync::{Mutex, RwLock, mpsc};
use tokio::time;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::daemon::{Daemon, DaemonCommand, DaemonEvent};
use crate::dag::DependencyGraph;
use crate::health::HealthMonitor;
use crate::process::ProcessManager;
use crate::safety::SafetyInterceptor;

/// Main supervisor that coordinates all system components
#[derive(Clone)]
pub struct Supervisor {
    /// Reference to the daemon
    daemon: Arc<Daemon>,
    /// Command receiver (wrapped in Arc<Mutex> so Supervisor can be Clone)
    command_rx: Arc<Mutex<mpsc::Receiver<DaemonCommand>>>,
    /// Event sender for broadcasting events
    event_tx: mpsc::Sender<DaemonEvent>,
    /// Task handles for service monitoring
    service_tasks: Arc<Mutex<HashMap<String, tokio::task::JoinHandle<()>>>>,
    /// Restart tracking for each service
    restart_tracking: Arc<RwLock<HashMap<String, RestartTracker>>>,
    /// Shutdown signal
    shutdown: Arc<Mutex<bool>>,
    /// Background task handle
    supervisor_task: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

/// Tracks restart attempts and timing for a service
#[derive(Debug, Clone)]
struct RestartTracker {
    /// Number of restart attempts made
    attempts: u32,
    /// Time of last restart
    last_restart: Instant,
    /// Whether we've given up on restarting
    gave_up: bool,
}

impl Supervisor {
    /// Create a new supervisor
    pub async fn new(daemon: Arc<Daemon>) -> Result<Self, String> {
        let command_rx = daemon
            .take_command_receiver()
            .await
            .map_err(|e| format!("Failed to get command receiver: {}", e))?;
        let event_tx = daemon.event_sender();

        // Initialize restart tracking
        let mut restart_tracking = HashMap::new();
        for service_name in daemon.config().services.keys() {
            restart_tracking.insert(
                service_name.clone(),
                RestartTracker {
                    attempts: 0,
                    last_restart: Instant::now(),
                    gave_up: false,
                },
            );
        }

        Ok(Self {
            daemon: daemon.clone(),
            command_rx: Arc::new(Mutex::new(command_rx)),
            event_tx: event_tx.clone(),
            service_tasks: Arc::new(Mutex::new(HashMap::new())),
            restart_tracking: Arc::new(RwLock::new(restart_tracking)),
            shutdown: Arc::new(Mutex::new(false)),
            supervisor_task: Arc::new(Mutex::new(None)),
        })
    }

    /// Start the supervisor
    pub async fn start(&mut self) -> Result<(), String> {
        info!("Starting supervisor");

        // Start health monitoring
        self.daemon
            .health_monitor()
            .start()
            .await
            .map_err(|e| format!("Failed to start health monitor: {}", e))?;

        // Clone self for the spawned task (Supervisor is now Clone)
        let mut supervisor_clone = self.clone();

        // Start the main supervisor loop in a background task
        let handle = tokio::spawn(async move {
            if let Err(e) = supervisor_clone.run().await {
                error!("Supervisor error: {}", e);
            }
        });

        // Store the handle so we can stop it during shutdown
        *self.supervisor_task.lock().await = Some(handle);

        info!("Supervisor started");
        Ok(())
    }

    /// Main supervisor loop
    async fn run(&mut self) -> Result<(), String> {
        info!("Supervisor running");

        // Start any auto-start services (those with no dependencies)
        self.start_ready_services().await?;

        // Main command processing loop
        loop {
            // Check for shutdown first
            if *self.shutdown.lock().await {
                break;
            }

            // Try to receive a command
            let command = {
                let mut rx = self.command_rx.lock().await;
                rx.recv().await
            };

            match command {
                Some(cmd) => {
                    if let Err(e) = self.handle_command(cmd).await {
                        error!("Error handling command: {}", e);
                    }
                }
                None => {
                    // Channel closed, exit loop
                    break;
                }
            }
        }

        info!("Supervisor loop ended");
        Ok(())
    }

    /// Handle a command from IPC server or internal events
    async fn handle_command(&self, command: DaemonCommand) -> Result<(), String> {
        match command {
            DaemonCommand::StartService(service_name) => self.start_service(&service_name).await,
            DaemonCommand::StopService(service_name) => self.stop_service(&service_name).await,
            DaemonCommand::RestartService(service_name) => {
                self.restart_service(&service_name).await
            }
            DaemonCommand::EmergencyStop => self.handle_emergency_stop().await,
            DaemonCommand::GetStatus => {
                // Status is handled by IPC server directly
                Ok(())
            }
            DaemonCommand::GetServiceStatus(service_name) => {
                // Status is handled by IPC server directly
                Ok(())
            }
            DaemonCommand::GetServiceLogs(service_name, lines) => {
                // Logs are handled by IPC server directly
                Ok(())
            }
            DaemonCommand::Shutdown => self.shutdown().await,
        }
    }

    /// Start a service with dependency resolution
    async fn start_service(&self, service_name: &str) -> Result<(), String> {
        info!("Starting service '{}'", service_name);

        // Check if service exists
        let service_config = self
            .daemon
            .get_service_config(service_name)
            .ok_or_else(|| format!("Service '{}' not found", service_name))?;

        // Check if already running
        if self.daemon.process_manager().is_running(service_name).await {
            warn!("Service '{}' is already running", service_name);
            return Ok(());
        }

        // Check if safety-stopped
        if self
            .daemon
            .safety_interceptor()
            .is_safety_stopped(service_name)
            .await
        {
            return Err(format!(
                "Service '{}' is safety-stopped and cannot be started",
                service_name
            ));
        }

        // Check dependencies
        let dependencies_satisfied = self.check_dependencies(service_name).await?;
        if !dependencies_satisfied {
            info!(
                "Dependencies not satisfied for '{}', queuing for later start",
                service_name
            );
            // In a full implementation, we would queue this service to start later
            return Ok(());
        }

        // Update state to Starting
        self.daemon
            .update_service_state(service_name, ServiceState::Starting, None, None)
            .await?;

        // Spawn the service
        self.spawn_service(service_name, service_config).await?;

        Ok(())
    }

    /// Check if all dependencies for a service are satisfied
    async fn check_dependencies(&self, service_name: &str) -> Result<bool, String> {
        // Get service states asynchronously
        let service_states = self.daemon.get_service_states().await;

        let get_state = move |dep_name: &str| {
            service_states
                .get(dep_name)
                .map(|status| status.state.clone())
                .unwrap_or(ServiceState::Stopped)
        };

        Ok(self
            .daemon
            .dependency_graph()
            .are_dependencies_satisfied(service_name, get_state))
    }

    /// Spawn a service process
    async fn spawn_service(
        &self,
        service_name: &str,
        config: &ServiceConfig,
    ) -> Result<(), String> {
        info!(
            "Spawning service '{}' with command: {}",
            service_name, config.command
        );

        let pid = self
            .daemon
            .process_manager()
            .spawn_process(
                service_name.to_string(),
                config.command.clone(),
                config.environment.clone(),
                config.working_directory.clone(),
            )
            .await?;

        // Update state to Running
        self.daemon
            .update_service_state(service_name, ServiceState::Running, Some(pid), None)
            .await?;

        // Start monitoring task for this service
        self.start_service_monitoring(service_name.to_string(), config.clone())
            .await?;

        Ok(())
    }

    /// Start monitoring task for a service
    async fn start_service_monitoring(
        &self,
        service_name: String,
        _config: ServiceConfig,
    ) -> Result<(), String> {
        let _daemon = self.daemon.clone();
        let _restart_tracking = self.restart_tracking.clone();
        let service_tasks = self.service_tasks.clone();
        let service_name_for_task = service_name.clone();
        let service_name_for_map = service_name.clone();

        let handle = tokio::spawn(async move {
            // In a full implementation, this would:
            // 1. Monitor process health
            // 2. Handle restarts according to policy
            // 3. Update service state
            // 4. Handle failures and safety interception

            // For now, just log that monitoring started
            info!("Started monitoring for service '{}'", service_name_for_task);

            // Keep the task alive
            tokio::time::sleep(Duration::from_secs(3600)).await;
        });

        let mut tasks = service_tasks.lock().await;
        tasks.insert(service_name_for_map, handle);

        Ok(())
    }

    /// Stop a service
    async fn stop_service(&self, service_name: &str) -> Result<(), String> {
        info!("Stopping service '{}'", service_name);

        // Check if service exists
        if self.daemon.get_service_config(service_name).is_none() {
            return Err(format!("Service '{}' not found", service_name));
        }

        // Update state to Stopping
        self.daemon
            .update_service_state(service_name, ServiceState::Stopping, None, None)
            .await?;

        // Stop the process
        self.daemon
            .process_manager()
            .stop_process(service_name)
            .await?;

        // Update state to Stopped
        self.daemon
            .update_service_state(service_name, ServiceState::Stopped, None, None)
            .await?;

        // Stop monitoring task
        self.stop_service_monitoring(service_name).await;

        Ok(())
    }

    /// Stop monitoring for a service
    async fn stop_service_monitoring(&self, service_name: &str) {
        let mut tasks = self.service_tasks.lock().await;
        if let Some(handle) = tasks.remove(service_name) {
            handle.abort();
            debug!("Stopped monitoring for service '{}'", service_name);
        }
    }

    /// Restart a service
    async fn restart_service(&self, service_name: &str) -> Result<(), String> {
        info!("Restarting service '{}'", service_name);

        // Stop if running
        if self.daemon.process_manager().is_running(service_name).await {
            self.stop_service(service_name).await?;
        }

        // Wait a bit
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Start again
        self.start_service(service_name).await
    }

    /// Handle emergency stop
    async fn handle_emergency_stop(&self) -> Result<(), String> {
        info!("Emergency stop requested");

        // Trigger safety interceptor's emergency procedure
        let reason = "Manual emergency stop command".to_string();

        self.daemon
            .safety_interceptor()
            .trigger_emergency_stop(&reason)
            .await
    }

    /// Start all services that are ready (dependencies satisfied)
    async fn start_ready_services(&self) -> Result<(), String> {
        info!("Starting ready services");

        let services = self
            .daemon
            .config()
            .services
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        let mut errors = Vec::new();

        for service_name in services {
            // Skip if already running
            if self
                .daemon
                .process_manager()
                .is_running(&service_name)
                .await
            {
                continue;
            }

            // Skip if safety-stopped
            if self
                .daemon
                .safety_interceptor()
                .is_safety_stopped(&service_name)
                .await
            {
                continue;
            }

            // Try to start
            if let Err(e) = self.start_service(&service_name).await {
                errors.push(format!("{}: {}", service_name, e));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(format!("Errors starting services: {}", errors.join(", ")))
        }
    }

    /// Handle a service failure
    async fn handle_service_failure(
        &self,
        service_name: &str,
        exit_code: Option<i32>,
        failure_reason: &str,
    ) -> Result<(), String> {
        info!(
            "Handling failure of service '{}': {}",
            service_name, failure_reason
        );

        // Get service config
        let service_config = self
            .daemon
            .get_service_config(service_name)
            .ok_or_else(|| format!("Service '{}' not found", service_name))?;

        // Update state to Failed
        self.daemon
            .update_service_state(service_name, ServiceState::Failed, None, exit_code)
            .await?;

        // Stop monitoring task
        self.stop_service_monitoring(service_name).await;

        // Handle via safety interceptor
        self.daemon
            .safety_interceptor()
            .handle_service_failure(service_name, service_config, exit_code, failure_reason)
            .await?;

        // Check restart policy
        if let Some(restart_policy) = &service_config.restart_policy {
            if self.should_restart(service_name, restart_policy).await {
                info!("Restarting service '{}' per restart policy", service_name);
                self.restart_service(service_name).await?;
            }
        }

        Ok(())
    }

    /// Determine if a service should be restarted based on policy
    async fn should_restart(
        &self,
        service_name: &str,
        policy: &krill_common::model::RestartPolicy,
    ) -> bool {
        let mut tracking = self.restart_tracking.write().await;
        let tracker = tracking
            .entry(service_name.to_string())
            .or_insert_with(|| RestartTracker {
                attempts: 0,
                last_restart: Instant::now(),
                gave_up: false,
            });

        if tracker.gave_up {
            return false;
        }

        // Check condition
        match policy.condition {
            RestartPolicyCondition::Always => true,
            RestartPolicyCondition::Never => false,
            RestartPolicyCondition::OnFailure => {
                // Check max attempts
                if tracker.attempts >= policy.max_attempts {
                    tracker.gave_up = true;
                    warn!(
                        "Service '{}' reached max restart attempts ({})",
                        service_name, policy.max_attempts
                    );
                    return false;
                }

                // Check delay
                let elapsed = tracker.last_restart.elapsed();
                if elapsed < Duration::from_secs(policy.delay_sec as u64) {
                    let remaining = Duration::from_secs(policy.delay_sec as u64) - elapsed;
                    debug!(
                        "Waiting {}s before restarting '{}'",
                        remaining.as_secs(),
                        service_name
                    );
                    tokio::time::sleep(remaining).await;
                }

                tracker.attempts += 1;
                tracker.last_restart = Instant::now();
                true
            }
        }
    }

    /// Reset restart tracking for a service (after successful start)
    async fn reset_restart_tracking(&self, service_name: &str) {
        let mut tracking = self.restart_tracking.write().await;
        if let Some(tracker) = tracking.get_mut(service_name) {
            tracker.attempts = 0;
            tracker.gave_up = false;
            debug!("Reset restart tracking for service '{}'", service_name);
        }
    }

    /// Gracefully shutdown the supervisor
    pub async fn shutdown(&self) -> Result<(), String> {
        info!("Shutting down supervisor");

        // Set shutdown flag
        {
            let mut shutdown = self.shutdown.lock().await;
            *shutdown = true;
        }

        // Stop all service monitoring tasks
        {
            let mut tasks = self.service_tasks.lock().await;
            for (service_name, handle) in tasks.drain() {
                handle.abort();
                debug!("Stopped monitoring for service '{}'", service_name);
            }
        }

        // Stop health monitor
        self.daemon.health_monitor().stop().await?;

        // Stop supervisor task
        if let Some(handle) = self.supervisor_task.lock().await.take() {
            handle.abort();
        }

        info!("Supervisor shutdown complete");
        Ok(())
    }
}

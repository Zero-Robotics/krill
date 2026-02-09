// Daemon Orchestrator - Coordinates all services using DAG

use crate::runner::{RunnerError, ServiceRunner, ServiceState};
use krill_common::{
    DagError, Dependency, DependencyCondition, DependencyGraph, KrillConfig, ServiceStatus,
};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio::time::{self, Duration};
use tracing::{debug, error, info, warn};

#[derive(Debug, Error)]
pub enum OrchestratorError {
    #[error("DAG error: {0}")]
    DagError(#[from] DagError),

    #[error("Service '{0}' not found")]
    ServiceNotFound(String),

    #[error("Runner error: {0}")]
    RunnerError(#[from] RunnerError),

    #[error("Shutdown in progress")]
    ShuttingDown,
}

pub type ServiceEvent = (String, ServiceStatus);
pub type LogLine = (String, String); // (service_name, line)

pub struct Orchestrator {
    config: Arc<KrillConfig>,
    dag: Arc<DependencyGraph>,
    runners: Arc<RwLock<HashMap<String, Arc<Mutex<ServiceRunner>>>>>,
    event_tx: mpsc::UnboundedSender<ServiceEvent>,
    log_tx: Option<mpsc::UnboundedSender<LogLine>>,
    shutdown: Arc<Mutex<bool>>,
}

impl Orchestrator {
    pub fn new(
        config: KrillConfig,
        event_tx: mpsc::UnboundedSender<ServiceEvent>,
    ) -> Result<Self, OrchestratorError> {
        Self::with_log_tx(config, event_tx, None)
    }

    pub fn with_log_tx(
        config: KrillConfig,
        event_tx: mpsc::UnboundedSender<ServiceEvent>,
        log_tx: Option<mpsc::UnboundedSender<LogLine>>,
    ) -> Result<Self, OrchestratorError> {
        // Build dependency graph
        let deps_map: HashMap<String, Vec<Dependency>> = config
            .services
            .iter()
            .map(|(name, svc)| (name.clone(), svc.dependencies.clone()))
            .collect();

        let dag = DependencyGraph::new(&deps_map)?;

        // Create runners for all services
        let mut runners = HashMap::new();
        for (name, svc_config) in &config.services {
            let runner = ServiceRunner::new(
                name.clone(),
                config.name.clone(),
                svc_config.clone(),
                config.env.clone(),
            );
            runners.insert(name.clone(), Arc::new(Mutex::new(runner)));
        }

        Ok(Self {
            config: Arc::new(config),
            dag: Arc::new(dag),
            runners: Arc::new(RwLock::new(runners)),
            event_tx,
            log_tx,
            shutdown: Arc::new(Mutex::new(false)),
        })
    }

    /// Start all services in DAG order
    pub async fn start_all(&self) -> Result<(), OrchestratorError> {
        info!("Starting all services in DAG order");

        let startup_order = self.dag.startup_order()?;

        // Start all services concurrently - dependencies are handled by start_when_ready
        let mut handles = vec![];

        for service_name in startup_order {
            let self_clone = self.clone_for_task();
            let service_name = service_name.clone();

            let handle =
                tokio::spawn(async move { self_clone.start_when_ready(&service_name).await });

            handles.push(handle);
        }

        // Wait for all services to start
        for handle in handles {
            match handle.await {
                Ok(Ok(())) => {
                    // Service started successfully
                }
                Ok(Err(e)) => {
                    error!("Failed to start service: {}", e);
                }
                Err(e) => {
                    error!("Service task panicked: {}", e);
                }
            }
        }

        info!("All services started");
        Ok(())
    }

    /// Start a service when its dependencies are ready
    async fn start_when_ready(&self, service_name: &str) -> Result<(), OrchestratorError> {
        debug!("Waiting for dependencies of '{}'", service_name);

        let service_config = self
            .config
            .services
            .get(service_name)
            .ok_or_else(|| OrchestratorError::ServiceNotFound(service_name.to_string()))?;

        // Wait for each dependency to meet its condition
        for dep in &service_config.dependencies {
            let dep_service = dep.service_name();
            let condition = dep.condition();

            loop {
                if *self.shutdown.lock().await {
                    return Err(OrchestratorError::ShuttingDown);
                }

                let dep_state = {
                    let runners = self.runners.read().await;
                    let runner = runners.get(dep_service).ok_or_else(|| {
                        OrchestratorError::ServiceNotFound(dep_service.to_string())
                    })?;
                    let state = runner.lock().await.state();
                    state
                };

                let condition_met = match condition {
                    DependencyCondition::Started => matches!(
                        dep_state,
                        ServiceState::Running | ServiceState::Healthy | ServiceState::Degraded
                    ),
                    DependencyCondition::Healthy => matches!(dep_state, ServiceState::Healthy),
                };

                if condition_met {
                    debug!(
                        "Dependency '{}' of '{}' satisfied ({:?})",
                        dep_service, service_name, condition
                    );
                    break;
                }

                // Wait a bit before checking again
                time::sleep(Duration::from_millis(100)).await;
            }
        }

        // All dependencies satisfied, start the service
        info!("Starting service '{}'", service_name);

        let runners = self.runners.read().await;
        let runner = runners
            .get(service_name)
            .ok_or_else(|| OrchestratorError::ServiceNotFound(service_name.to_string()))?;

        let mut runner_guard = runner.lock().await;
        runner_guard.start().await?;

        // Take stdout/stderr handles and spawn output capture tasks
        if let Some(stdout) = runner_guard.take_stdout() {
            self.spawn_output_reader(service_name.to_string(), stdout, false);
        }
        if let Some(stderr) = runner_guard.take_stderr() {
            self.spawn_output_reader(service_name.to_string(), stderr, true);
        }

        // Send event
        let status = runner_guard.get_status();
        let _ = self.event_tx.send((service_name.to_string(), status));

        // Start monitoring task
        drop(runner_guard);
        drop(runners);
        self.start_monitoring_task(service_name);

        Ok(())
    }

    /// Spawn a task to read output from a process stream
    fn spawn_output_reader<R>(&self, service_name: String, reader: R, is_stderr: bool)
    where
        R: tokio::io::AsyncRead + Unpin + Send + 'static,
    {
        let log_tx = self.log_tx.clone();
        let stream_type = if is_stderr { "stderr" } else { "stdout" };

        tokio::spawn(async move {
            let mut lines = BufReader::new(reader).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                // Log to tracing
                if is_stderr {
                    warn!("[{}] {}", service_name, line);
                } else {
                    info!("[{}] {}", service_name, line);
                }

                // Send to log channel if available
                if let Some(ref tx) = log_tx {
                    let _ = tx.send((service_name.clone(), line));
                }
            }
            debug!("[{}] {} stream closed", service_name, stream_type);
        });
    }

    /// Start a monitoring task for a service
    fn start_monitoring_task(&self, service_name: &str) {
        let service_name = service_name.to_string();
        let self_clone = self.clone_for_task();

        tokio::spawn(async move {
            self_clone.monitor_service(&service_name).await;
        });
    }

    /// Monitor a service and handle failures/restarts
    async fn monitor_service(&self, service_name: &str) {
        let mut interval = time::interval(Duration::from_secs(1));

        loop {
            interval.tick().await;

            if *self.shutdown.lock().await {
                break;
            }

            let runners = self.runners.read().await;
            let runner = match runners.get(service_name) {
                Some(r) => r,
                None => break,
            };

            let mut runner_guard = runner.lock().await;

            // Check if process is still running
            if !runner_guard.is_running() {
                let exit_code = runner_guard.get_exit_code();
                let current_state = runner_guard.state();

                // Skip if service is already being handled (stopping, stopped, or failed)
                if matches!(
                    current_state,
                    ServiceState::Stopping | ServiceState::Stopped | ServiceState::Failed
                ) {
                    continue;
                }

                warn!(
                    "Service '{}' process exited with code: {:?}",
                    service_name, exit_code
                );

                let should_restart = runner_guard.should_restart(exit_code);

                runner_guard.mark_failed();
                let status = runner_guard.get_status();
                let _ = self.event_tx.send((service_name.to_string(), status));

                if should_restart {
                    info!("Restarting service '{}'", service_name);

                    // Wait for restart delay
                    let restart_delay = runner_guard.config.policy.restart_delay;
                    drop(runner_guard);
                    drop(runners);

                    time::sleep(restart_delay).await;

                    // Try to restart
                    if let Err(e) = self.start_when_ready(service_name).await {
                        error!("Failed to restart service '{}': {}", service_name, e);
                    }
                } else {
                    info!("Service '{}' will not be restarted", service_name);

                    // Check if this is a critical service
                    let is_critical = self
                        .config
                        .services
                        .get(service_name)
                        .map(|s| s.critical)
                        .unwrap_or(false);

                    if is_critical {
                        error!(
                            "Critical service '{}' failed, initiating emergency stop",
                            service_name
                        );
                        drop(runner_guard);
                        drop(runners);
                        self.emergency_stop().await;
                    } else {
                        // Cascade failure to dependents
                        drop(runner_guard);
                        drop(runners);
                        self.cascade_failure(service_name).await;
                    }

                    break;
                }
            }
        }
    }

    /// Handle cascading failure
    async fn cascade_failure(&self, failed_service: &str) {
        info!("Cascading failure from '{}'", failed_service);

        let dependents = self.dag.cascade_failure(failed_service);

        for dependent in dependents {
            info!("Stopping dependent service '{}'", dependent);

            let runners = self.runners.read().await;
            if let Some(runner) = runners.get(&dependent) {
                let mut runner_guard = runner.lock().await;
                if let Err(e) = runner_guard.stop().await {
                    error!("Failed to stop dependent '{}': {}", dependent, e);
                }

                let status = runner_guard.get_status();
                let _ = self.event_tx.send((dependent.clone(), status));
            }
        }
    }

    /// Emergency stop all services
    async fn emergency_stop(&self) {
        error!("EMERGENCY STOP - Stopping all services immediately");

        *self.shutdown.lock().await = true;

        let runners = self.runners.read().await;
        for (name, runner) in runners.iter() {
            let mut runner_guard = runner.lock().await;
            info!("Emergency stopping service '{}'", name);
            if let Err(e) = runner_guard.stop().await {
                error!("Error during emergency stop of '{}': {}", name, e);
            }
        }
    }

    /// Graceful shutdown in reverse DAG order
    pub async fn shutdown(&self) -> Result<(), OrchestratorError> {
        info!("Starting graceful shutdown");

        *self.shutdown.lock().await = true;

        let shutdown_order = self.dag.shutdown_order()?;

        // Stop services sequentially in reverse dependency order
        for service_name in shutdown_order {
            let runners = self.runners.read().await;
            if let Some(runner) = runners.get(&service_name) {
                let runner = runner.clone();
                let event_tx = self.event_tx.clone();
                let name = service_name.clone();

                drop(runners); // Release read lock before spawning

                let mut runner_guard = runner.lock().await;
                info!("Stopping service '{}'", name);

                if let Err(e) = runner_guard.stop().await {
                    error!("Failed to stop '{}': {}", name, e);
                }

                let status = runner_guard.get_status();
                let _ = event_tx.send((name, status));
            }
        }

        info!("Graceful shutdown complete");
        Ok(())
    }

    /// Process heartbeat from a service
    pub async fn process_heartbeat(
        &self,
        service_name: &str,
        status: ServiceStatus,
        _metadata: HashMap<String, String>,
    ) -> Result<(), OrchestratorError> {
        let runners = self.runners.read().await;
        let runner = runners
            .get(service_name)
            .ok_or_else(|| OrchestratorError::ServiceNotFound(service_name.to_string()))?
            .clone();
        drop(runners);

        let mut runner_guard = runner.lock().await;

        // Update the service health based on the heartbeat status
        // Healthy and Running statuses indicate the service is responsive
        let is_healthy = matches!(status, ServiceStatus::Healthy | ServiceStatus::Running);
        runner_guard.update_health(is_healthy);

        // Broadcast the actual status update to clients
        let updated_status = runner_guard.get_status();
        let _ = self
            .event_tx
            .send((service_name.to_string(), updated_status));

        Ok(())
    }

    /// Get status of all services
    pub async fn get_snapshot(&self) -> HashMap<String, krill_common::ServiceSnapshot> {
        let mut snapshot = HashMap::new();
        let runners = self.runners.read().await;

        for (name, runner) in runners.iter() {
            let runner_guard = runner.lock().await;
            let service_config = self.config.services.get(name);

            // Calculate uptime if service has started
            let uptime = runner_guard.uptime();

            // Get dependencies
            let dependencies = service_config
                .map(|cfg| {
                    cfg.dependencies
                        .iter()
                        .map(|d| d.service_name().to_string())
                        .collect()
                })
                .unwrap_or_default();

            // Get GPU usage
            let uses_gpu = service_config.map(|cfg| cfg.gpu).unwrap_or(false);

            // Get critical flag
            let critical = service_config.map(|cfg| cfg.critical).unwrap_or(false);

            // Get restart policy
            let restart_policy = service_config
                .map(|cfg| format!("{:?}", cfg.policy.restart))
                .unwrap_or_else(|| "Unknown".to_string());

            let max_restarts = service_config
                .map(|cfg| cfg.policy.max_restarts)
                .unwrap_or(0);

            snapshot.insert(
                name.clone(),
                krill_common::ServiceSnapshot {
                    status: runner_guard.get_status(),
                    pid: runner_guard.pid(),
                    uid: runner_guard.uid().to_string(),
                    uptime,
                    restart_count: runner_guard.restart_count(),
                    last_error: None,
                    namespace: runner_guard.namespace().to_string(),
                    executor_type: runner_guard.executor_type().to_string(),
                    dependencies,
                    uses_gpu,
                    critical,
                    restart_policy,
                    max_restarts,
                },
            );
        }

        snapshot
    }

    /// Stop a specific service
    pub async fn stop_service(&self, name: &str) -> Result<(), OrchestratorError> {
        let runners = self.runners.read().await;
        let runner = runners
            .get(name)
            .ok_or_else(|| OrchestratorError::ServiceNotFound(name.to_string()))?
            .clone();
        drop(runners);

        let mut runner_guard = runner.lock().await;
        info!("Stopping service '{}'", name);

        // Send "stopping" status
        let _ = self
            .event_tx
            .send((name.to_string(), krill_common::ServiceStatus::Stopping));

        runner_guard.stop().await?;

        let status = runner_guard.get_status();
        let _ = self.event_tx.send((name.to_string(), status));

        info!("Service '{}' stopped", name);

        Ok(())
    }

    /// Restart a specific service
    pub async fn restart_service(&self, name: &str) -> Result<(), OrchestratorError> {
        let runners = self.runners.read().await;
        let runner = runners
            .get(name)
            .ok_or_else(|| OrchestratorError::ServiceNotFound(name.to_string()))?
            .clone();
        drop(runners);

        let mut runner_guard = runner.lock().await;
        info!("Restarting service '{}'", name);

        // Send "restarting" status (we use Stopping as intermediate state)
        let _ = self
            .event_tx
            .send((name.to_string(), krill_common::ServiceStatus::Stopping));

        // Stop first
        if let Err(e) = runner_guard.stop().await {
            warn!("Error stopping service '{}' during restart: {}", name, e);
        }

        // Send "stopped" status
        let _ = self
            .event_tx
            .send((name.to_string(), krill_common::ServiceStatus::Stopped));

        // Brief pause to make the state change visible
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Send "starting" status
        let _ = self
            .event_tx
            .send((name.to_string(), krill_common::ServiceStatus::Starting));

        // Increment restart count manually since we're doing a manual restart
        runner_guard.increment_restart_count();

        // Start again
        runner_guard.start().await?;

        // Take stdout/stderr handles and spawn output capture tasks
        if let Some(stdout) = runner_guard.take_stdout() {
            self.spawn_output_reader(name.to_string(), stdout, false);
        }
        if let Some(stderr) = runner_guard.take_stderr() {
            self.spawn_output_reader(name.to_string(), stderr, true);
        }

        let status = runner_guard.get_status();
        let _ = self.event_tx.send((name.to_string(), status));

        // Start monitoring task for the restarted service
        drop(runner_guard);
        self.start_monitoring_task(name);

        info!("Service '{}' restarted successfully", name);

        Ok(())
    }

    fn clone_for_task(&self) -> Self {
        Self {
            config: Arc::clone(&self.config),
            dag: Arc::clone(&self.dag),
            runners: Arc::clone(&self.runners),
            event_tx: self.event_tx.clone(),
            log_tx: self.log_tx.clone(),
            shutdown: Arc::clone(&self.shutdown),
        }
    }
}

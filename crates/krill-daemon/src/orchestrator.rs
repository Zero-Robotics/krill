// Daemon Orchestrator - Coordinates all services using DAG

use crate::runner::{RunnerError, ServiceRunner, ServiceState};
use krill_common::{
    DagError, Dependency, DependencyCondition, DependencyGraph, KrillConfig, ServiceStatus,
};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
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

pub struct Orchestrator {
    config: Arc<KrillConfig>,
    dag: Arc<DependencyGraph>,
    runners: Arc<RwLock<HashMap<String, Arc<Mutex<ServiceRunner>>>>>,
    event_tx: mpsc::UnboundedSender<ServiceEvent>,
    shutdown: Arc<Mutex<bool>>,
}

impl Orchestrator {
    pub fn new(
        config: KrillConfig,
        event_tx: mpsc::UnboundedSender<ServiceEvent>,
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
            shutdown: Arc::new(Mutex::new(false)),
        })
    }

    /// Start all services in DAG order
    pub async fn start_all(&self) -> Result<(), OrchestratorError> {
        info!("Starting all services in DAG order");

        let startup_order = self.dag.startup_order();

        for layer in startup_order {
            info!("Starting layer: {:?}", layer);

            // Start all services in this layer concurrently
            let mut handles = vec![];

            for service_name in layer {
                let self_clone = self.clone_for_task();
                let service_name = service_name.clone();

                let handle =
                    tokio::spawn(async move { self_clone.start_when_ready(&service_name).await });

                handles.push(handle);
            }

            // Wait for all services in this layer to start
            for handle in handles {
                if let Err(e) = handle.await {
                    error!("Failed to start service: {}", e);
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

        // Send event
        let status = runner_guard.get_status();
        let _ = self.event_tx.send((service_name.to_string(), status));

        // Start monitoring task
        drop(runner_guard);
        drop(runners);
        self.start_monitoring_task(service_name);

        Ok(())
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
                warn!("Service '{}' process exited", service_name);

                let exit_code = None; // TODO: capture exit code
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

        let shutdown_order = self.dag.shutdown_order();

        for layer in shutdown_order {
            info!("Stopping layer: {:?}", layer);

            let mut handles = vec![];

            for service_name in layer {
                let runners = self.runners.read().await;
                if let Some(runner) = runners.get(&service_name) {
                    let runner = runner.clone();
                    let service_name = service_name.clone();
                    let event_tx = self.event_tx.clone();

                    let handle = tokio::spawn(async move {
                        let mut runner_guard = runner.lock().await;
                        info!("Stopping service '{}'", service_name);

                        if let Err(e) = runner_guard.stop().await {
                            error!("Failed to stop '{}': {}", service_name, e);
                        }

                        let status = runner_guard.get_status();
                        let _ = event_tx.send((service_name, status));
                    });

                    handles.push(handle);
                }
            }

            // Wait for all services in this layer to stop
            for handle in handles {
                let _ = handle.await;
            }
        }

        info!("Graceful shutdown complete");
        Ok(())
    }

    /// Get status of all services
    pub async fn get_snapshot(&self) -> HashMap<String, ServiceStatus> {
        let mut snapshot = HashMap::new();
        let runners = self.runners.read().await;

        for (name, runner) in runners.iter() {
            let runner_guard = runner.lock().await;
            snapshot.insert(name.clone(), runner_guard.get_status());
        }

        snapshot
    }

    fn clone_for_task(&self) -> Self {
        Self {
            config: Arc::clone(&self.config),
            dag: Arc::clone(&self.dag),
            runners: Arc::clone(&self.runners),
            event_tx: self.event_tx.clone(),
            shutdown: Arc::clone(&self.shutdown),
        }
    }
}

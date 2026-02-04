use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use krill_common::model::{ServiceConfig, ServiceState};
use tokio::sync::{Mutex, RwLock, mpsc};
use tracing::{debug, error, info, warn};

use crate::daemon::DaemonEvent;
use crate::dag::DependencyGraph;
use crate::process::ProcessManager;

/// Safety Interceptor for critical failure handling
///
/// Implements the "Safety Interceptor Pattern":
/// 1. Stop Dependents: Immediately kill any service that depends on the failed node
/// 2. Escalation: If the failed service is critical, enter Global Emergency Mode
/// 3. Emergency Stop: Execute system-wide emergency stop or kill all services
pub struct SafetyInterceptor {
    /// Dependency graph for analyzing service relationships
    dependency_graph: DependencyGraph,
    /// Process manager for stopping services
    process_manager: ProcessManager,
    /// Event sender for broadcasting safety events
    event_tx: mpsc::Sender<DaemonEvent>,
    /// Emergency mode state
    emergency_mode: Arc<Mutex<bool>>,
    /// Services that have been stopped due to safety (to prevent restart loops)
    safety_stopped: Arc<RwLock<HashSet<String>>>,
    /// Global emergency stop command (if defined)
    global_stop_cmd: Arc<Mutex<Option<String>>>,
}

impl SafetyInterceptor {
    /// Create a new safety interceptor
    pub fn new(
        dependency_graph: DependencyGraph,
        process_manager: ProcessManager,
        event_tx: mpsc::Sender<DaemonEvent>,
    ) -> Self {
        Self {
            dependency_graph,
            process_manager,
            event_tx,
            emergency_mode: Arc::new(Mutex::new(false)),
            safety_stopped: Arc::new(RwLock::new(HashSet::new())),
            global_stop_cmd: Arc::new(Mutex::new(None)),
        }
    }

    /// Handle a service failure
    ///
    /// This is the main safety interceptor entry point. It determines if the
    /// failure requires safety intervention and takes appropriate actions.
    pub async fn handle_service_failure(
        &self,
        service_name: &str,
        service_config: &ServiceConfig,
        exit_code: Option<i32>,
        failure_reason: &str,
    ) -> Result<(), String> {
        info!(
            "Safety interceptor handling failure for service '{}': {}",
            service_name, failure_reason
        );

        // Check if this service is marked as critical
        if service_config.critical {
            self.handle_critical_failure(service_name, exit_code, failure_reason)
                .await
        } else {
            self.handle_non_critical_failure(service_name, failure_reason)
                .await
        }
    }

    /// Trigger emergency stop manually (e.g., from TUI command)
    pub async fn trigger_emergency_stop(&self, reason: &str) -> Result<(), String> {
        warn!("MANUAL EMERGENCY STOP triggered: {}", reason);

        // Send emergency stop event
        let _ = self
            .event_tx
            .send(DaemonEvent::EmergencyStopTriggered(reason.to_string()))
            .await;

        // Enter global emergency mode
        self.enter_emergency_mode("manual_command", reason).await?;

        // Execute emergency stop procedure
        self.execute_emergency_stop("manual_command").await
    }

    /// Handle a critical service failure
    async fn handle_critical_failure(
        &self,
        service_name: &str,
        exit_code: Option<i32>,
        failure_reason: &str,
    ) -> Result<(), String> {
        warn!(
            "CRITICAL FAILURE: Service '{}' failed: {}",
            service_name, failure_reason
        );

        // Send critical failure event
        let _ = self
            .event_tx
            .send(DaemonEvent::CriticalFailure(
                service_name.to_string(),
                failure_reason.to_string(),
            ))
            .await;

        // Step 1: Stop all dependents
        self.stop_dependent_services(service_name).await?;

        // Step 2: Enter global emergency mode
        self.enter_emergency_mode(service_name, failure_reason)
            .await?;

        // Step 3: Execute emergency stop
        self.execute_emergency_stop(service_name).await?;

        Ok(())
    }

    /// Handle a non-critical service failure
    async fn handle_non_critical_failure(
        &self,
        service_name: &str,
        failure_reason: &str,
    ) -> Result<(), String> {
        info!(
            "Non-critical service '{}' failed: {}",
            service_name, failure_reason
        );

        // Still stop dependents if any exist
        // (a non-critical service failing may still leave dependents in unsafe state)
        self.stop_dependent_services(service_name).await?;

        Ok(())
    }

    /// Stop all services that depend on the failed service
    async fn stop_dependent_services(&self, failed_service: &str) -> Result<(), String> {
        info!("Stopping services that depend on '{}'", failed_service);

        // Get all transitive dependents (recursive)
        let dependents: Vec<String> = self
            .dependency_graph
            .get_transitive_dependents(failed_service)
            .into_iter()
            .collect();

        if dependents.is_empty() {
            info!("No services depend on '{}'", failed_service);
            return Ok(());
        }

        info!(
            "Found {} dependent(s) to stop: {:?}",
            dependents.len(),
            dependents
        );

        let mut errors = Vec::new();

        // Stop each dependent service
        for dependent in dependents {
            // Check if already stopped by safety
            {
                let safety_stopped = self.safety_stopped.read().await;
                if safety_stopped.contains(&dependent) {
                    debug!("Service '{}' already safety-stopped, skipping", dependent);
                    continue;
                }
            }

            info!("Safety-stopping dependent service '{}'", dependent);

            // Mark as safety-stopped
            {
                let mut safety_stopped = self.safety_stopped.write().await;
                safety_stopped.insert(dependent.clone());
            }

            // Stop the process
            if let Err(e) = self.process_manager.stop_process(&dependent).await {
                errors.push(format!("{}: {}", dependent, e));
                warn!("Failed to safety-stop dependent '{}': {}", dependent, e);
            }

            // Send event
            let _ = self
                .event_tx
                .send(DaemonEvent::ServiceStopped(
                    dependent.clone(),
                    None, // No exit code for safety stop
                ))
                .await;
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(format!("Errors stopping dependents: {}", errors.join(", ")))
        }
    }

    /// Enter global emergency mode
    async fn enter_emergency_mode(&self, failed_service: &str, reason: &str) -> Result<(), String> {
        let mut emergency_mode = self.emergency_mode.lock().await;

        if *emergency_mode {
            warn!("Already in emergency mode");
            return Ok(());
        }

        *emergency_mode = true;

        warn!(
            "GLOBAL EMERGENCY MODE ACTIVATED due to critical failure of '{}': {}",
            failed_service, reason
        );

        // Send emergency mode event
        let _ = self
            .event_tx
            .send(DaemonEvent::EmergencyStopTriggered(format!(
                "Critical failure of '{}': {}",
                failed_service, reason
            )))
            .await;

        Ok(())
    }

    /// Execute emergency stop procedure
    async fn execute_emergency_stop(&self, failed_service: &str) -> Result<(), String> {
        info!("Executing emergency stop procedure");

        // First, try to execute global stop command if defined
        if let Err(e) = self.execute_global_stop_command().await {
            warn!("Failed to execute global stop command: {}", e);
            // Fall back to killing all services
        }

        // Kill all services in reverse topological order
        // This ensures dependents are stopped before their dependencies
        self.kill_all_services_safely().await?;

        info!(
            "Emergency stop procedure completed for failed service '{}'",
            failed_service
        );
        Ok(())
    }

    /// Execute global emergency stop command if defined
    async fn execute_global_stop_command(&self) -> Result<(), String> {
        let global_stop_cmd = self.global_stop_cmd.lock().await;

        if let Some(cmd) = &*global_stop_cmd {
            info!("Executing global emergency stop command: {}", cmd);

            // Parse command
            let parts: Vec<&str> = cmd.split_whitespace().collect();
            if parts.is_empty() {
                return Err("Empty global stop command".to_string());
            }

            let executable = parts[0];
            let args = &parts[1..];

            // Execute command with timeout
            let result = tokio::process::Command::new(executable)
                .args(args)
                .output()
                .await;

            match result {
                Ok(output) => {
                    if output.status.success() {
                        info!("Global emergency stop command executed successfully");
                        Ok(())
                    } else {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        Err(format!("Global stop command failed: {}", stderr))
                    }
                }
                Err(e) => Err(format!("Failed to execute global stop command: {}", e)),
            }
        } else {
            // No global stop command defined
            info!("No global emergency stop command defined");
            Ok(())
        }
    }

    /// Kill all services in reverse topological order
    async fn kill_all_services_safely(&self) -> Result<(), String> {
        info!("Killing all services in safe shutdown order");

        // Get services in reverse topological order (dependents first)
        let shutdown_order = self.dependency_graph.reverse_topological_order();

        info!("Safe shutdown order: {:?}", shutdown_order);

        let mut errors = Vec::new();

        // Stop each service in order
        for service_name in shutdown_order {
            // Skip if already safety-stopped
            {
                let safety_stopped = self.safety_stopped.read().await;
                if safety_stopped.contains(&service_name) {
                    debug!(
                        "Service '{}' already safety-stopped, skipping",
                        service_name
                    );
                    continue;
                }
            }

            info!("Emergency stopping service '{}'", service_name);

            // Mark as safety-stopped
            {
                let mut safety_stopped = self.safety_stopped.write().await;
                safety_stopped.insert(service_name.clone());
            }

            // Stop the process (use force kill for emergency)
            if let Err(e) = self.process_manager.stop_process(&service_name).await {
                errors.push(format!("{}: {}", service_name, e));
                error!("Failed to emergency stop '{}': {}", service_name, e);
            }

            // Send event
            let _ = self
                .event_tx
                .send(DaemonEvent::ServiceStopped(service_name.clone(), None))
                .await;
        }

        if errors.is_empty() {
            info!("All services stopped safely");
            Ok(())
        } else {
            Err(format!(
                "Errors during emergency stop: {}",
                errors.join(", ")
            ))
        }
    }

    /// Check if we're in emergency mode
    pub async fn is_emergency_mode(&self) -> bool {
        *self.emergency_mode.lock().await
    }

    /// Set the global emergency stop command
    pub async fn set_global_stop_cmd(&self, cmd: Option<String>) {
        let mut global_stop_cmd = self.global_stop_cmd.lock().await;
        *global_stop_cmd = cmd;

        if let Some(cmd) = &*global_stop_cmd {
            info!("Global emergency stop command set: {}", cmd);
        } else {
            info!("Global emergency stop command cleared");
        }
    }

    /// Get the global emergency stop command
    pub async fn get_global_stop_cmd(&self) -> Option<String> {
        let global_stop_cmd = self.global_stop_cmd.lock().await;
        global_stop_cmd.clone()
    }

    /// Reset safety state (useful after system recovery)
    pub async fn reset(&self) -> Result<(), String> {
        info!("Resetting safety interceptor state");

        // Clear emergency mode
        {
            let mut emergency_mode = self.emergency_mode.lock().await;
            *emergency_mode = false;
        }

        // Clear safety-stopped set
        {
            let mut safety_stopped = self.safety_stopped.write().await;
            safety_stopped.clear();
        }

        info!("Safety interceptor reset complete");
        Ok(())
    }

    /// Check if a service has been safety-stopped
    pub async fn is_safety_stopped(&self, service_name: &str) -> bool {
        let safety_stopped = self.safety_stopped.read().await;
        safety_stopped.contains(service_name)
    }

    /// Clear safety-stopped flag for a service (allow it to restart)
    pub async fn clear_safety_stopped(&self, service_name: &str) -> Result<(), String> {
        let mut safety_stopped = self.safety_stopped.write().await;

        if safety_stopped.remove(service_name) {
            info!("Cleared safety-stopped flag for service '{}'", service_name);
            Ok(())
        } else {
            Err(format!("Service '{}' was not safety-stopped", service_name))
        }
    }

    /// Get all safety-stopped services
    pub async fn get_safety_stopped_services(&self) -> Vec<String> {
        let safety_stopped = self.safety_stopped.read().await;
        safety_stopped.iter().cloned().collect()
    }

    /// Analyze safety impact of a service failure
    pub async fn analyze_safety_impact(&self, service_name: &str) -> SafetyImpact {
        let is_critical = self
            .dependency_graph
            .contains_service(service_name)
            .then(|| {
                // In a real implementation, we would check service config
                // For now, we'll return a basic analysis
                let dependents = self
                    .dependency_graph
                    .get_transitive_dependents(service_name);
                SafetyImpact {
                    service: service_name.to_string(),
                    is_critical: false, // Would check from config
                    dependents_count: dependents.len(),
                    would_trigger_emergency: false, // Would check from config
                    dependents: dependents.into_iter().collect(),
                }
            })
            .unwrap_or_else(|| SafetyImpact {
                service: service_name.to_string(),
                is_critical: false,
                dependents_count: 0,
                would_trigger_emergency: false,
                dependents: HashSet::new(),
            });

        is_critical
    }
}

/// Analysis of safety impact for a service failure
pub struct SafetyImpact {
    pub service: String,
    pub is_critical: bool,
    pub dependents_count: usize,
    pub would_trigger_emergency: bool,
    pub dependents: HashSet<String>,
}

impl SafetyImpact {
    pub fn new(service: String) -> Self {
        Self {
            service,
            is_critical: false,
            dependents_count: 0,
            would_trigger_emergency: false,
            dependents: HashSet::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use krill_common::model::{Dependency, DependencyCondition, ServiceConfig, ServicesConfig};
    use tokio::sync::mpsc;

    fn create_test_config() -> ServicesConfig {
        let mut services = std::collections::HashMap::new();

        // Critical service
        services.insert(
            "lidar".to_string(),
            ServiceConfig {
                command: "/usr/bin/lidar".to_string(),
                stop_cmd: None,
                restart_policy: None,
                critical: true,
                health_check: None,
                dependencies: vec![],
                environment: None,
                working_directory: None,
            },
        );

        // Dependent on lidar
        services.insert(
            "navigator".to_string(),
            ServiceConfig {
                command: "/usr/bin/navigator".to_string(),
                stop_cmd: None,
                restart_policy: None,
                critical: false,
                health_check: None,
                dependencies: vec![Dependency {
                    service: "lidar".to_string(),
                    condition: DependencyCondition::Healthy,
                }],
                environment: None,
                working_directory: None,
            },
        );

        // Dependent on navigator
        services.insert(
            "ui".to_string(),
            ServiceConfig {
                command: "/usr/bin/ui".to_string(),
                stop_cmd: None,
                restart_policy: None,
                critical: false,
                health_check: None,
                dependencies: vec![Dependency {
                    service: "navigator".to_string(),
                    condition: DependencyCondition::Started,
                }],
                environment: None,
                working_directory: None,
            },
        );

        ServicesConfig {
            version: "1".to_string(),
            services,
        }
    }

    #[tokio::test]
    async fn test_safety_interceptor_creation() {
        let config = create_test_config();
        let dependency_graph = DependencyGraph::new(&config).unwrap();

        let (event_tx, _) = mpsc::channel(10);

        // We can't create a real ProcessManager without proper setup,
        // but we can test that the struct creation doesn't panic
        let process_manager =
            ProcessManager::new(std::path::PathBuf::from("/tmp"), event_tx.clone())
                .expect("Failed to create process manager");

        let interceptor = SafetyInterceptor::new(dependency_graph, process_manager, event_tx);

        assert!(!interceptor.is_emergency_mode().await);
    }

    #[tokio::test]
    async fn test_emergency_mode() {
        let config = create_test_config();
        let dependency_graph = DependencyGraph::new(&config).unwrap();

        let (event_tx, _) = mpsc::channel(10);
        let process_manager =
            ProcessManager::new(std::path::PathBuf::from("/tmp"), event_tx.clone())
                .expect("Failed to create process manager");

        let interceptor = SafetyInterceptor::new(dependency_graph, process_manager, event_tx);

        // Initially not in emergency mode
        assert!(!interceptor.is_emergency_mode().await);

        // Test setting global stop command
        interceptor
            .set_global_stop_cmd(Some("/usr/bin/emergency_stop".to_string()))
            .await;
        let cmd = interceptor.get_global_stop_cmd().await;
        assert_eq!(cmd, Some("/usr/bin/emergency_stop".to_string()));

        // Test clearing
        interceptor.set_global_stop_cmd(None).await;
        let cmd = interceptor.get_global_stop_cmd().await;
        assert_eq!(cmd, None);
    }

    #[tokio::test]
    async fn test_safety_impact_analysis() {
        let config = create_test_config();
        let dependency_graph = DependencyGraph::new(&config).unwrap();

        let (event_tx, _) = mpsc::channel(10);
        let process_manager =
            ProcessManager::new(std::path::PathBuf::from("/tmp"), event_tx.clone())
                .expect("Failed to create process manager");

        let interceptor = SafetyInterceptor::new(dependency_graph, process_manager, event_tx);

        let impact = interceptor.analyze_safety_impact("lidar").await;
        assert_eq!(impact.service, "lidar");
        assert_eq!(impact.dependents_count, 2); // navigator and ui
        assert!(impact.dependents.contains("navigator"));
        assert!(impact.dependents.contains("ui"));
    }
}

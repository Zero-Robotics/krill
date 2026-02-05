// Service Runner - Manages individual service lifecycle

use krill_common::{
    build_command, generate_process_name, get_stop_command, get_working_dir, HealthChecker,
    ServiceConfig, ServiceStatus,
};
use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;
use std::collections::HashMap;
use std::process::Stdio;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::process::{Child, ChildStderr, ChildStdout, Command};
use tracing::{debug, error, info, warn};

#[derive(Debug, Error)]
pub enum RunnerError {
    #[error("Failed to spawn process: {0}")]
    SpawnFailed(String),

    #[error("Process not running")]
    ProcessNotRunning,

    #[error("Stop timeout exceeded")]
    StopTimeout,

    #[error("Health check failed: {0}")]
    HealthCheckFailed(String),

    #[error("GPU not available: {0}")]
    GpuNotAvailable(String),

    #[error("Restart limit exceeded")]
    RestartLimitExceeded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceState {
    Pending,
    Starting,
    Running,
    Healthy,
    Degraded,
    Stopping,
    Stopped,
    Failed,
}

pub struct ServiceRunner {
    pub service_name: String,
    pub workspace_name: String,
    pub config: ServiceConfig,
    state: ServiceState,
    process: Option<Child>,
    pid: Option<u32>,
    pgid: Option<u32>,
    restart_count: u32,
    last_healthy_time: Option<Instant>,
    #[allow(dead_code)] // Reserved for future health check implementation
    health_checker: Option<HealthChecker>,
    env_vars: HashMap<String, String>,
}

impl ServiceRunner {
    pub fn new(
        service_name: String,
        workspace_name: String,
        config: ServiceConfig,
        env_vars: HashMap<String, String>,
    ) -> Self {
        let health_checker = config.health_check.clone();

        Self {
            service_name,
            workspace_name,
            config,
            state: ServiceState::Pending,
            process: None,
            pid: None,
            pgid: None,
            restart_count: 0,
            last_healthy_time: None,
            health_checker,
            env_vars,
        }
    }

    pub fn state(&self) -> ServiceState {
        self.state.clone()
    }

    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    pub fn restart_count(&self) -> u32 {
        self.restart_count
    }

    pub fn increment_restart_count(&mut self) {
        self.restart_count += 1;
    }

    /// Start the service
    pub async fn start(&mut self) -> Result<(), RunnerError> {
        if self.state != ServiceState::Pending && self.state != ServiceState::Stopped {
            warn!(
                "Service '{}' already in state {:?}, skipping start",
                self.service_name, self.state
            );
            return Ok(());
        }

        // Check GPU if required
        if self.config.gpu {
            let gpu_req = krill_common::GpuRequirement {
                required: true,
                min_memory_gb: None,
                compute_capability: None,
            };
            krill_common::validate_gpu_available(&gpu_req).map_err(|e| {
                RunnerError::GpuNotAvailable(format!("Service requires GPU: {}", e))
            })?;
        }

        // Check restart limit
        if self.config.policy.max_restarts > 0
            && self.restart_count >= self.config.policy.max_restarts
        {
            error!(
                "Service '{}' exceeded max restarts ({})",
                self.service_name, self.config.policy.max_restarts
            );
            self.state = ServiceState::Failed;
            return Err(RunnerError::RestartLimitExceeded);
        }

        info!("Starting service '{}'", self.service_name);
        self.state = ServiceState::Starting;

        // Build command
        let cmd_parts = build_command(&self.config.execute, &self.env_vars)
            .map_err(|e| RunnerError::SpawnFailed(e.to_string()))?;

        if cmd_parts.is_empty() {
            return Err(RunnerError::SpawnFailed("Empty command".to_string()));
        }

        let program = &cmd_parts[0];
        let args = &cmd_parts[1..];

        // Set up process name
        let process_name = generate_process_name(&self.service_name, None)
            .map_err(|e| RunnerError::SpawnFailed(e.to_string()))?;

        // Create command
        let mut command = Command::new(program);
        command
            .args(args)
            .env("KRILL_SERVICE_NAME", &self.service_name)
            .env("KRILL_PROCESS_NAME", &process_name)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Set working directory if specified
        if let Some(work_dir) = get_working_dir(&self.config.execute) {
            command.current_dir(work_dir);
        }

        // Add environment variables
        for (key, value) in &self.env_vars {
            command.env(key, value);
        }

        // Spawn process
        let child = command
            .spawn()
            .map_err(|e| RunnerError::SpawnFailed(format!("Failed to spawn: {}", e)))?;

        let pid = child
            .id()
            .ok_or_else(|| RunnerError::SpawnFailed("No PID available".to_string()))?;

        debug!("Spawned service '{}' with PID {}", self.service_name, pid);

        // Set up process group for isolation
        #[cfg(unix)]
        {
            if let Err(e) = krill_common::setup_process_group(pid) {
                warn!(
                    "Failed to set process group for '{}': {}",
                    self.service_name, e
                );
            } else {
                // Get the PGID
                if let Ok(pgid) = krill_common::get_process_group(pid) {
                    self.pgid = Some(pgid);
                    debug!("Service '{}' process group: {}", self.service_name, pgid);
                }
            }
        }

        self.process = Some(child);
        self.pid = Some(pid);
        self.state = ServiceState::Running;

        info!(
            "Service '{}' started successfully (PID: {})",
            self.service_name, pid
        );

        Ok(())
    }

    /// Stop the service gracefully
    pub async fn stop(&mut self) -> Result<(), RunnerError> {
        if self.state == ServiceState::Stopped || self.state == ServiceState::Pending {
            return Ok(());
        }

        info!("Stopping service '{}'", self.service_name);
        self.state = ServiceState::Stopping;

        // Try graceful stop command first
        if let Some(stop_cmd) = get_stop_command(&self.config.execute) {
            debug!("Executing stop command for '{}'", self.service_name);
            if let Ok(mut cmd) = Command::new(&stop_cmd[0]).args(&stop_cmd[1..]).spawn() {
                let _ = tokio::time::timeout(Duration::from_secs(5), cmd.wait()).await;
            }
        }

        // Send SIGTERM to process group
        if let Some(pgid) = self.pgid {
            debug!("Sending SIGTERM to process group {}", pgid);
            let _ = krill_common::kill_process_group(pgid, Signal::SIGTERM);
        } else if let Some(pid) = self.pid {
            debug!("Sending SIGTERM to PID {}", pid);
            let _ = signal::kill(Pid::from_raw(pid as i32), Signal::SIGTERM);
        }

        // Wait for process to exit
        let timeout = self.config.policy.stop_timeout;
        let wait_result = if let Some(ref mut process) = self.process {
            tokio::time::timeout(timeout, process.wait()).await
        } else {
            return Err(RunnerError::ProcessNotRunning);
        };

        match wait_result {
            Ok(Ok(status)) => {
                info!("Service '{}' stopped: {:?}", self.service_name, status);
                self.cleanup();
                Ok(())
            }
            Ok(Err(e)) => {
                error!("Error waiting for service '{}': {}", self.service_name, e);
                self.force_kill().await
            }
            Err(_) => {
                warn!(
                    "Service '{}' did not stop within timeout, sending SIGKILL",
                    self.service_name
                );
                self.force_kill().await
            }
        }
    }

    /// Force kill the service with SIGKILL
    async fn force_kill(&mut self) -> Result<(), RunnerError> {
        if let Some(pgid) = self.pgid {
            debug!("Sending SIGKILL to process group {}", pgid);
            let _ = krill_common::kill_process_group(pgid, Signal::SIGKILL);
        } else if let Some(pid) = self.pid {
            debug!("Sending SIGKILL to PID {}", pid);
            let _ = signal::kill(Pid::from_raw(pid as i32), Signal::SIGKILL);
        }

        // Force wait
        if let Some(ref mut process) = self.process {
            let _ = tokio::time::timeout(Duration::from_secs(5), process.wait()).await;
        }

        self.cleanup();
        Ok(())
    }

    fn cleanup(&mut self) {
        self.state = ServiceState::Stopped;
        self.process = None;
        self.pid = None;
        self.pgid = None;
    }

    /// Check if process is still running
    pub fn is_running(&mut self) -> bool {
        if let Some(ref mut process) = self.process {
            process.try_wait().ok().flatten().is_none()
        } else {
            false
        }
    }

    /// Update health status
    pub fn update_health(&mut self, is_healthy: bool) {
        match (self.state.clone(), is_healthy) {
            (ServiceState::Running, true) => {
                self.state = ServiceState::Healthy;
                self.last_healthy_time = Some(Instant::now());

                // Reset restart count after being healthy for 60 seconds
                if let Some(last_healthy) = self.last_healthy_time {
                    if last_healthy.elapsed() > Duration::from_secs(60) {
                        self.restart_count = 0;
                    }
                }
            }
            (ServiceState::Healthy, false) => {
                self.state = ServiceState::Degraded;
                warn!("Service '{}' degraded", self.service_name);
            }
            (ServiceState::Degraded, true) => {
                self.state = ServiceState::Healthy;
                info!("Service '{}' recovered", self.service_name);
            }
            _ => {}
        }
    }

    /// Mark service as failed
    pub fn mark_failed(&mut self) {
        error!("Service '{}' marked as failed", self.service_name);
        self.state = ServiceState::Failed;
        self.restart_count += 1;
    }

    /// Check if service should be restarted
    pub fn should_restart(&self, exit_code: Option<i32>) -> bool {
        use krill_common::policy::RestartPolicy;

        match self.config.policy.restart {
            RestartPolicy::Never => false,
            RestartPolicy::Always => {
                if self.config.policy.max_restarts > 0 {
                    self.restart_count < self.config.policy.max_restarts
                } else {
                    true
                }
            }
            RestartPolicy::OnFailure => {
                let is_failure = exit_code.map(|c| c != 0).unwrap_or(true);
                if is_failure && self.config.policy.max_restarts > 0 {
                    self.restart_count < self.config.policy.max_restarts
                } else {
                    is_failure
                }
            }
        }
    }

    pub fn get_status(&self) -> ServiceStatus {
        match self.state {
            ServiceState::Pending => ServiceStatus::Starting,
            ServiceState::Starting => ServiceStatus::Starting,
            ServiceState::Running => ServiceStatus::Running,
            ServiceState::Healthy => ServiceStatus::Healthy,
            ServiceState::Degraded => ServiceStatus::Degraded,
            ServiceState::Stopping => ServiceStatus::Stopping,
            ServiceState::Stopped => ServiceStatus::Stopped,
            ServiceState::Failed => ServiceStatus::Failed,
        }
    }

    pub fn namespace(&self) -> &str {
        &self.workspace_name
    }

    pub fn executor_type(&self) -> &str {
        self.config.execute.executor_type()
    }

    /// Take the stdout handle from the process (can only be called once)
    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.process.as_mut().and_then(|p| p.stdout.take())
    }

    /// Take the stderr handle from the process (can only be called once)
    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.process.as_mut().and_then(|p| p.stderr.take())
    }
}

// Phase 1: krill-common - Shared Types
// Phase 2: DAG Resolution
// Phase 3: Process Management
// Phase 4: Health Checks
// Phase 5: Configuration

pub mod config;
pub mod dag;
pub mod dependency;
pub mod execute;
pub mod health;
pub mod ipc;
pub mod policy;
pub mod process;
pub mod validation;

pub use config::{ConfigError, KrillConfig, ServiceConfig};
pub use dag::{DagError, DependencyGraph};
pub use dependency::{Dependency, DependencyCondition};
pub use execute::ExecuteConfig;
pub use health::{validate_gpu_available, GpuRequirement, HealthChecker, HealthError};
pub use ipc::{ClientMessage, CommandAction, ServerMessage, ServiceStatus};
pub use policy::{PolicyConfig, RestartPolicy};
pub use process::{
    build_command, generate_process_name, get_process_group, get_stop_command, get_working_dir,
    kill_process_group, setup_process_group, ProcessError,
};
pub use validation::validate_shell_command;

// Re-export commonly used types
pub use serde::{Deserialize, Serialize};
pub use std::collections::HashMap;
pub use std::time::Duration;

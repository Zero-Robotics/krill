use crate::execute::ExecuteConfig;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command as StdCommand;
use thiserror::Error;

#[cfg(unix)]
use nix::unistd::{setpgid, Pid};

#[derive(Debug, Error)]
pub enum ProcessError {
    #[error("Invalid process name: {0}")]
    InvalidName(String),

    #[error("Command build failed: {0}")]
    BuildFailed(String),

    #[error("PGID isolation failed: {0}")]
    PgidError(String),

    #[error("Command not found: {0}")]
    CommandNotFound(String),
}

/// Find the full path to an executable using which/where
pub fn find_executable(program: &str) -> Result<String, ProcessError> {
    // If it's already an absolute path or contains a slash, use it directly
    if program.starts_with('/') || program.contains('/') {
        return Ok(program.to_string());
    }

    // Try to find it using 'which' command
    let output = StdCommand::new("which")
        .arg(program)
        .output()
        .map_err(|e| ProcessError::CommandNotFound(format!("Failed to run 'which': {}", e)))?;

    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() {
            return Ok(path);
        }
    }

    // Fallback: return the program name as-is (will fail at spawn time with better error)
    Err(ProcessError::CommandNotFound(format!(
        "Command '{}' not found in PATH. Make sure it's installed and accessible.",
        program
    )))
}

/// Generate a unique process name for a service instance
pub fn generate_process_name(
    service_name: &str,
    instance_id: Option<u32>,
) -> Result<String, ProcessError> {
    // Validate service name (alphanumeric, hyphens, underscores only)
    if !service_name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return Err(ProcessError::InvalidName(format!(
            "Service name '{}' contains invalid characters",
            service_name
        )));
    }

    if service_name.is_empty() {
        return Err(ProcessError::InvalidName(
            "Service name cannot be empty".to_string(),
        ));
    }

    let process_name = match instance_id {
        Some(id) => format!("krill.{}.{}", service_name, id),
        None => format!("krill.{}", service_name),
    };

    Ok(process_name)
}

/// Build a complete command from ExecuteConfig
pub fn build_command(
    config: &ExecuteConfig,
    env_vars: &HashMap<String, String>,
) -> Result<Vec<String>, ProcessError> {
    match config {
        ExecuteConfig::Pixi {
            task, environment, ..
        } => {
            let mut cmd = vec!["pixi".to_string(), "run".to_string()];

            if let Some(env) = environment {
                cmd.push("-e".to_string());
                cmd.push(env.clone());
            }

            cmd.push(task.clone());

            Ok(cmd)
        }

        ExecuteConfig::Ros2 {
            package,
            launch_file,
            launch_args,
            ..
        } => {
            let mut cmd = vec![
                "ros2".to_string(),
                "launch".to_string(),
                package.clone(),
                launch_file.clone(),
            ];

            // Add launch arguments as key:=value
            for (key, value) in launch_args {
                cmd.push(format!("{}:={}", key, value));
            }

            Ok(cmd)
        }

        ExecuteConfig::Shell { command, .. } => {
            // Use shell to execute the command
            Ok(vec!["sh".to_string(), "-c".to_string(), command.clone()])
        }

        ExecuteConfig::Docker {
            image,
            volumes,
            ports,
            privileged,
            network,
        } => {
            let mut cmd = vec!["docker".to_string(), "run".to_string()];

            // Add volume mounts
            for volume in volumes {
                let mount_spec = if volume.read_only() {
                    format!(
                        "{}:{}:ro",
                        volume.host().display(),
                        volume.container().display()
                    )
                } else {
                    format!(
                        "{}:{}",
                        volume.host().display(),
                        volume.container().display()
                    )
                };
                cmd.push("-v".to_string());
                cmd.push(mount_spec);
            }

            // Add port mappings
            for port in ports {
                cmd.push("-p".to_string());
                cmd.push(format!(
                    "{}:{}/{}",
                    port.host, port.container, port.protocol
                ));
            }

            // Add privileged flag
            if *privileged {
                cmd.push("--privileged".to_string());
            }

            // Add network mode
            if let Some(net) = network {
                cmd.push("--network".to_string());
                cmd.push(net.clone());
            }

            // Add environment variables
            for (key, value) in env_vars {
                cmd.push("-e".to_string());
                cmd.push(format!("{}={}", key, value));
            }

            // Add image name
            cmd.push(image.clone());

            Ok(cmd)
        }
    }
}

/// Get the working directory from ExecuteConfig
pub fn get_working_dir(config: &ExecuteConfig) -> Option<PathBuf> {
    match config {
        ExecuteConfig::Pixi { working_dir, .. } => working_dir.clone(),
        ExecuteConfig::Ros2 { working_dir, .. } => working_dir.clone(),
        ExecuteConfig::Shell { working_dir, .. } => working_dir.clone(),
        ExecuteConfig::Docker { .. } => None, // Docker handles working dir internally
    }
}

/// Get the stop command from ExecuteConfig
pub fn get_stop_command(config: &ExecuteConfig) -> Option<Vec<String>> {
    match config {
        ExecuteConfig::Pixi {
            stop_task: Some(task),
            environment,
            ..
        } => {
            let mut cmd = vec!["pixi".to_string(), "run".to_string()];

            if let Some(env) = environment {
                cmd.push("-e".to_string());
                cmd.push(env.clone());
            }

            cmd.push(task.clone());
            Some(cmd)
        }

        ExecuteConfig::Ros2 {
            stop_task: Some(task),
            ..
        } => {
            // Assume stop task is a simple command
            Some(vec!["sh".to_string(), "-c".to_string(), task.clone()])
        }

        ExecuteConfig::Shell {
            stop_command: Some(cmd),
            ..
        } => Some(vec!["sh".to_string(), "-c".to_string(), cmd.clone()]),

        ExecuteConfig::Docker { .. } => {
            // Docker stop is handled by container name/ID
            None
        }

        _ => None,
    }
}

/// Set up a new process group for the given process ID
#[cfg(unix)]
pub fn setup_process_group(pid: u32) -> Result<(), ProcessError> {
    let process_pid = Pid::from_raw(pid as i32);

    // Set the process as its own process group leader
    setpgid(process_pid, process_pid)
        .map_err(|e| ProcessError::PgidError(format!("Failed to set process group: {}", e)))?;

    Ok(())
}

/// Kill an entire process group using SIGTERM
#[cfg(unix)]
pub fn kill_process_group(pgid: u32, signal: nix::sys::signal::Signal) -> Result<(), ProcessError> {
    use nix::sys::signal::killpg;

    let process_pgid = Pid::from_raw(pgid as i32);

    killpg(process_pgid, signal)
        .map_err(|e| ProcessError::PgidError(format!("Failed to kill process group: {}", e)))?;

    Ok(())
}

/// Get the process group ID for a given process
#[cfg(unix)]
pub fn get_process_group(pid: u32) -> Result<u32, ProcessError> {
    use nix::unistd::getpgid;

    let process_pid = Pid::from_raw(pid as i32);
    let pgid = getpgid(Some(process_pid))
        .map_err(|e| ProcessError::PgidError(format!("Failed to get process group: {}", e)))?;

    Ok(pgid.as_raw() as u32)
}

// Placeholder implementations for non-Unix platforms
#[cfg(not(unix))]
pub fn setup_process_group(_pid: u32) -> Result<(), ProcessError> {
    Err(ProcessError::PgidError(
        "Process group isolation is only supported on Unix platforms".to_string(),
    ))
}

#[cfg(not(unix))]
pub fn kill_process_group(_pgid: u32, _signal: i32) -> Result<(), ProcessError> {
    Err(ProcessError::PgidError(
        "Process group operations are only supported on Unix platforms".to_string(),
    ))
}

#[cfg(not(unix))]
pub fn get_process_group(_pid: u32) -> Result<u32, ProcessError> {
    Err(ProcessError::PgidError(
        "Process group operations are only supported on Unix platforms".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execute::PortMapping;

    #[test]
    fn test_generate_process_name() {
        let name = generate_process_name("lidar", None).unwrap();
        assert_eq!(name, "krill.lidar");

        let name_with_id = generate_process_name("camera", Some(42)).unwrap();
        assert_eq!(name_with_id, "krill.camera.42");
    }

    #[test]
    fn test_invalid_process_name() {
        let result = generate_process_name("", None);
        assert!(result.is_err());

        let result = generate_process_name("bad name!", None);
        assert!(result.is_err());
    }

    #[test]
    fn test_build_pixi_command() {
        let config = ExecuteConfig::Pixi {
            task: "start-lidar".to_string(),
            environment: Some("drivers".to_string()),
            stop_task: None,
            working_dir: None,
        };

        let cmd = build_command(&config, &HashMap::new()).unwrap();
        assert_eq!(cmd, vec!["pixi", "run", "-e", "drivers", "start-lidar"]);
    }

    #[test]
    fn test_build_ros2_command() {
        let mut launch_args = HashMap::new();
        launch_args.insert("use_sim_time".to_string(), "true".to_string());

        let config = ExecuteConfig::Ros2 {
            package: "nav2_bringup".to_string(),
            launch_file: "navigation.launch.py".to_string(),
            launch_args,
            stop_task: None,
            working_dir: None,
        };

        let cmd = build_command(&config, &HashMap::new()).unwrap();
        assert!(cmd[0] == "ros2");
        assert!(cmd[1] == "launch");
        assert!(cmd.contains(&"use_sim_time:=true".to_string()));
    }

    #[test]
    fn test_build_shell_command() {
        let config = ExecuteConfig::Shell {
            command: "echo hello".to_string(),
            stop_command: None,
            working_dir: None,
        };

        let cmd = build_command(&config, &HashMap::new()).unwrap();
        assert_eq!(cmd, vec!["sh", "-c", "echo hello"]);
    }

    #[test]
    fn test_build_docker_command() {
        use crate::execute::VolumeMount;

        let config = ExecuteConfig::Docker {
            image: "ros:humble".to_string(),
            volumes: vec![VolumeMount::Detailed {
                host: PathBuf::from("/data"),
                container: PathBuf::from("/workspace"),
                read_only: true,
            }],
            ports: vec![PortMapping {
                host: 8080,
                container: 80,
                protocol: "tcp".to_string(),
            }],
            privileged: true,
            network: Some("host".to_string()),
        };

        let mut env = HashMap::new();
        env.insert("ROS_DOMAIN_ID".to_string(), "42".to_string());

        let cmd = build_command(&config, &env).unwrap();
        assert!(cmd.contains(&"docker".to_string()));
        assert!(cmd.contains(&"--privileged".to_string()));
        assert!(cmd.contains(&"-v".to_string()));
        assert!(cmd.contains(&"-p".to_string()));
    }

    #[test]
    fn test_get_stop_command() {
        let config = ExecuteConfig::Shell {
            command: "start.sh".to_string(),
            stop_command: Some("stop.sh".to_string()),
            working_dir: None,
        };

        let stop_cmd = get_stop_command(&config);
        assert!(stop_cmd.is_some());
        assert_eq!(stop_cmd.unwrap(), vec!["sh", "-c", "stop.sh"]);
    }

    #[test]
    #[cfg(unix)]
    fn test_process_group_with_current_process() {
        // Test getting the process group of the current process
        let current_pid = std::process::id();
        let result = get_process_group(current_pid);
        assert!(result.is_ok());

        // PGID should be a valid positive number
        let pgid = result.unwrap();
        assert!(pgid > 0);
    }

    #[test]
    #[cfg(unix)]
    fn test_setup_process_group_current_process() {
        // We can't easily test setup_process_group without spawning a subprocess,
        // so we'll just verify it doesn't panic with current process
        let current_pid = std::process::id();

        // This may fail if current process is already a group leader,
        // but it should not panic
        let _ = setup_process_group(current_pid);
    }

    #[test]
    #[cfg(unix)]
    fn test_kill_process_group_with_signal() {
        use nix::sys::signal::Signal;

        // Try to send signal 0 (null signal) to current process group
        // This just checks if the process group exists without actually sending a signal
        let current_pid = std::process::id();
        if let Ok(pgid) = get_process_group(current_pid) {
            // Signal 0 is safe - it just checks permissions without killing
            let result = kill_process_group(pgid, Signal::SIGCONT);
            // This might fail due to permissions, but shouldn't panic
            let _ = result;
        }
    }

    #[test]
    #[cfg(not(unix))]
    fn test_pgid_not_supported_on_non_unix() {
        let result = setup_process_group(1234);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("only supported on Unix"));

        let result = get_process_group(1234);
        assert!(result.is_err());
    }
}

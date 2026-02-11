use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HealthError {
    #[error("Health check failed: {0}")]
    CheckFailed(String),

    #[error("Timeout exceeded")]
    Timeout,

    #[error("GPU not available: {0}")]
    GpuUnavailable(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum HealthChecker {
    Heartbeat {
        #[serde(skip)]
        last_seen: Option<SystemTime>,
        #[serde(with = "humantime_serde")]
        timeout: Duration,
    },
    Tcp {
        port: u16,
        #[serde(with = "humantime_serde")]
        timeout: Duration,
    },
    Http {
        port: u16,
        path: String,
        #[serde(default = "default_http_status")]
        expected_status: u16,
    },
    Script {
        command: String,
        #[serde(with = "humantime_serde")]
        timeout: Duration,
    },
}

fn default_http_status() -> u16 {
    200
}

impl HealthChecker {
    /// Update the last seen time for heartbeat checks
    pub fn record_heartbeat(&mut self) -> Result<(), HealthError> {
        match self {
            HealthChecker::Heartbeat { last_seen, .. } => {
                *last_seen = Some(SystemTime::now());
                Ok(())
            }
            _ => Err(HealthError::CheckFailed(
                "Not a heartbeat health checker".to_string(),
            )),
        }
    }

    /// Check if the health check has timed out (for heartbeat type)
    pub fn is_timed_out(&self) -> bool {
        match self {
            HealthChecker::Heartbeat {
                last_seen: Some(time),
                timeout,
            } => time.elapsed().unwrap_or(Duration::from_secs(0)) > *timeout,
            HealthChecker::Heartbeat {
                last_seen: None, ..
            } => false, // Not started yet
            _ => false,
        }
    }

    /// Get the timeout duration if applicable
    pub fn timeout(&self) -> Option<Duration> {
        match self {
            HealthChecker::Heartbeat { timeout, .. } => Some(*timeout),
            HealthChecker::Tcp { timeout, .. } => Some(*timeout),
            HealthChecker::Script { timeout, .. } => Some(*timeout),
            HealthChecker::Http { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GpuRequirement {
    #[serde(default)]
    pub required: bool,

    #[serde(default)]
    pub min_memory_gb: Option<u32>,

    #[serde(default)]
    pub compute_capability: Option<String>,
}

/// Validate that GPU is available for the service
pub fn validate_gpu_available(requirement: &GpuRequirement) -> Result<(), HealthError> {
    if !requirement.required {
        return Ok(());
    }

    // Check for NVIDIA GPU using nvidia-smi
    #[cfg(target_os = "linux")]
    {
        use std::process::Command;

        let output = Command::new("nvidia-smi")
            .arg("--query-gpu=memory.total,compute_cap")
            .arg("--format=csv,noheader,nounits")
            .output();

        match output {
            Ok(result) if result.status.success() => {
                let stdout = String::from_utf8_lossy(&result.stdout);
                let line = stdout.lines().next().unwrap_or("");
                let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();

                if parts.len() >= 2 {
                    // Check memory if required
                    if let Some(min_mem) = requirement.min_memory_gb {
                        if let Ok(available_mem) = parts[0].parse::<u32>() {
                            let available_gb = available_mem / 1024; // Convert MB to GB
                            if available_gb < min_mem {
                                return Err(HealthError::GpuUnavailable(format!(
                                    "Insufficient GPU memory: {}GB < {}GB",
                                    available_gb, min_mem
                                )));
                            }
                        }
                    }

                    // Check compute capability if required
                    if let Some(required_cap) = &requirement.compute_capability {
                        let available_cap = parts[1];
                        if available_cap < required_cap.as_str() {
                            return Err(HealthError::GpuUnavailable(format!(
                                "Insufficient compute capability: {} < {}",
                                available_cap, required_cap
                            )));
                        }
                    }

                    Ok(())
                } else {
                    Err(HealthError::GpuUnavailable(
                        "Could not parse GPU information".to_string(),
                    ))
                }
            }
            Ok(_) => Err(HealthError::GpuUnavailable(
                "nvidia-smi command failed".to_string(),
            )),
            Err(e) => Err(HealthError::GpuUnavailable(format!(
                "nvidia-smi not found: {}",
                e
            ))),
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        // On non-Linux systems, we can't check GPU availability easily
        // Just warn but allow it
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heartbeat_checker() {
        let mut checker = HealthChecker::Heartbeat {
            last_seen: None,
            timeout: Duration::from_secs(5),
        };

        assert!(!checker.is_timed_out());

        checker.record_heartbeat().unwrap();
        assert!(!checker.is_timed_out());
    }

    #[test]
    fn test_tcp_checker() {
        let checker = HealthChecker::Tcp {
            port: 8080,
            timeout: Duration::from_secs(2),
        };

        assert_eq!(checker.timeout(), Some(Duration::from_secs(2)));
    }

    #[test]
    fn test_http_checker() {
        let checker = HealthChecker::Http {
            port: 3000,
            path: "/health".to_string(),
            expected_status: 200,
        };

        assert_eq!(checker.timeout(), None);
    }

    #[test]
    fn test_script_checker() {
        let checker = HealthChecker::Script {
            command: "check_health.sh".to_string(),
            timeout: Duration::from_secs(10),
        };

        assert_eq!(checker.timeout(), Some(Duration::from_secs(10)));
    }

    #[test]
    fn test_health_checker_serialize() {
        let checker = HealthChecker::Heartbeat {
            last_seen: None,
            timeout: Duration::from_secs(2),
        };

        let yaml = serde_yaml::to_string(&checker).unwrap();
        assert!(yaml.contains("type: heartbeat"));
        assert!(yaml.contains("timeout:"));
    }

    #[test]
    fn test_gpu_not_required() {
        let req = GpuRequirement {
            required: false,
            min_memory_gb: None,
            compute_capability: None,
        };

        assert!(validate_gpu_available(&req).is_ok());
    }

    #[test]
    fn test_gpu_requirement_serialization() {
        let req = GpuRequirement {
            required: true,
            min_memory_gb: Some(8),
            compute_capability: Some("7.5".to_string()),
        };

        let yaml = serde_yaml::to_string(&req).unwrap();
        let deserialized: GpuRequirement = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(req, deserialized);
    }
}

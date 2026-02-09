// Phase 1.3: Policy Block

use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyConfig {
    /// Restart policy: "always", "on-failure", "never"
    #[serde(default = "default_restart")]
    pub restart: RestartPolicy,

    /// Maximum number of restart attempts (0 = unlimited)
    #[serde(default)]
    pub max_restarts: u32,

    /// Delay between restart attempts
    #[serde(with = "humantime_serde", default = "default_restart_delay")]
    pub restart_delay: Duration,

    /// Timeout for graceful stop before SIGKILL
    #[serde(with = "humantime_serde", default = "default_stop_timeout")]
    pub stop_timeout: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RestartPolicy {
    Always,
    OnFailure,
    Never,
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            restart: default_restart(),
            max_restarts: 0,
            restart_delay: default_restart_delay(),
            stop_timeout: default_stop_timeout(),
        }
    }
}

fn default_restart() -> RestartPolicy {
    RestartPolicy::OnFailure
}

fn default_restart_delay() -> Duration {
    Duration::from_secs(5)
}

fn default_stop_timeout() -> Duration {
    Duration::from_secs(10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_policy() {
        let policy = PolicyConfig::default();
        assert_eq!(policy.restart, RestartPolicy::OnFailure);
        assert_eq!(policy.max_restarts, 0);
        assert_eq!(policy.restart_delay, Duration::from_secs(5));
    }

    #[test]
    fn test_deserialize_policy() {
        let yaml = r#"
restart: always
max_restarts: 3
restart_delay: 10s
stop_timeout: 30s
"#;
        let policy: PolicyConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(policy.restart, RestartPolicy::Always);
        assert_eq!(policy.max_restarts, 3);
        assert_eq!(policy.restart_delay, Duration::from_secs(10));
        assert_eq!(policy.stop_timeout, Duration::from_secs(30));
    }

    #[test]
    fn test_serialize_policy() {
        let policy = PolicyConfig {
            restart: RestartPolicy::Never,
            max_restarts: 5,
            restart_delay: Duration::from_secs(15),
            stop_timeout: Duration::from_secs(20),
        };

        let yaml = serde_yaml::to_string(&policy).unwrap();
        let deserialized: PolicyConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(policy, deserialized);
    }
}

// Phase 1.1: Execute Types (Tagged Enum)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ExecuteConfig {
    Pixi {
        task: String,
        #[serde(default)]
        environment: Option<String>,
        #[serde(default)]
        stop_task: Option<String>,
        #[serde(default)]
        working_dir: Option<PathBuf>,
    },
    Ros2 {
        package: String,
        launch_file: String,
        #[serde(default)]
        launch_args: HashMap<String, String>,
        #[serde(default)]
        stop_task: Option<String>,
        #[serde(default)]
        working_dir: Option<PathBuf>,
    },
    Shell {
        command: String,
        #[serde(default)]
        stop_command: Option<String>,
        #[serde(default)]
        working_dir: Option<PathBuf>,
    },
    Docker {
        image: String,
        #[serde(default)]
        volumes: Vec<VolumeMount>,
        #[serde(default)]
        ports: Vec<PortMapping>,
        #[serde(default)]
        privileged: bool,
        #[serde(default)]
        network: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VolumeMount {
    pub host: PathBuf,
    pub container: PathBuf,
    #[serde(default)]
    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortMapping {
    pub host: u16,
    pub container: u16,
    #[serde(default = "default_protocol")]
    pub protocol: String,
}

fn default_protocol() -> String {
    "tcp".to_string()
}

impl ExecuteConfig {
    pub fn executor_type(&self) -> &'static str {
        match self {
            ExecuteConfig::Pixi { .. } => "pixi",
            ExecuteConfig::Ros2 { .. } => "ros2",
            ExecuteConfig::Shell { .. } => "shell",
            ExecuteConfig::Docker { .. } => "docker",
        }
    }

    /// Resolve relative working_dir paths against a base directory
    pub fn resolve_working_dir(&mut self, base_dir: &std::path::Path) {
        let resolve = |working_dir: &mut Option<PathBuf>| {
            if let Some(ref mut wd) = working_dir {
                if wd.is_relative() {
                    *wd = base_dir.join(&wd);
                }
            }
        };

        match self {
            ExecuteConfig::Pixi { working_dir, .. } => resolve(working_dir),
            ExecuteConfig::Ros2 { working_dir, .. } => resolve(working_dir),
            ExecuteConfig::Shell { working_dir, .. } => resolve(working_dir),
            ExecuteConfig::Docker { .. } => {} // Docker doesn't have working_dir
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pixi_serialize() {
        let config = ExecuteConfig::Pixi {
            task: "start-lidar".to_string(),
            environment: Some("drivers".to_string()),
            stop_task: Some("stop-lidar".to_string()),
            working_dir: None,
        };

        let yaml = serde_yaml::to_string(&config).unwrap();
        assert!(yaml.contains("type: pixi"));
        assert!(yaml.contains("task: start-lidar"));
    }

    #[test]
    fn test_shell_deserialize() {
        let yaml = r#"
type: shell
command: "echo hello"
"#;
        let config: ExecuteConfig = serde_yaml::from_str(yaml).unwrap();
        match config {
            ExecuteConfig::Shell { command, .. } => {
                assert_eq!(command, "echo hello");
            }
            _ => panic!("Expected Shell variant"),
        }
    }

    #[test]
    fn test_docker_with_volumes() {
        let config = ExecuteConfig::Docker {
            image: "ros:humble".to_string(),
            volumes: vec![VolumeMount {
                host: PathBuf::from("/data"),
                container: PathBuf::from("/workspace"),
                read_only: false,
            }],
            ports: vec![],
            privileged: true,
            network: Some("host".to_string()),
        };

        let yaml = serde_yaml::to_string(&config).unwrap();
        let deserialized: ExecuteConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(config, deserialized);
    }
}

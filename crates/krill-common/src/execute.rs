use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
#[serde(deny_unknown_fields)]
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

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum VolumeMount {
    Detailed {
        host: PathBuf,
        container: PathBuf,
        #[serde(default)]
        read_only: bool,
    },
}

impl VolumeMount {
    pub fn host(&self) -> &PathBuf {
        match self {
            VolumeMount::Detailed { host, .. } => host,
        }
    }

    pub fn container(&self) -> &PathBuf {
        match self {
            VolumeMount::Detailed { container, .. } => container,
        }
    }

    pub fn read_only(&self) -> bool {
        match self {
            VolumeMount::Detailed { read_only, .. } => *read_only,
        }
    }
}

impl<'de> Deserialize<'de> for VolumeMount {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;

        let s = String::deserialize(deserializer)?;
        let parts: Vec<&str> = s.split(':').collect();

        match parts.len() {
            2 => Ok(VolumeMount::Detailed {
                host: PathBuf::from(parts[0]),
                container: PathBuf::from(parts[1]),
                read_only: false,
            }),
            3 if parts[2] == "ro" => Ok(VolumeMount::Detailed {
                host: PathBuf::from(parts[0]),
                container: PathBuf::from(parts[1]),
                read_only: true,
            }),
            _ => Err(Error::custom(format!(
                "Invalid volume mount format '{}'. Expected 'host:container' or 'host:container:ro'",
                s
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PortMapping {
    pub host: u16,
    pub container: u16,
    pub protocol: String,
}

impl<'de> Deserialize<'de> for PortMapping {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;

        let s = String::deserialize(deserializer)?;
        let parts: Vec<&str> = s.split(':').collect();

        if parts.len() != 2 {
            return Err(Error::custom(format!(
                "Invalid port mapping format '{}'. Expected 'host:container'",
                s
            )));
        }

        let host = parts[0]
            .parse::<u16>()
            .map_err(|_| Error::custom(format!("Invalid host port '{}'", parts[0])))?;

        let container = parts[1]
            .parse::<u16>()
            .map_err(|_| Error::custom(format!("Invalid container port '{}'", parts[1])))?;

        Ok(PortMapping {
            host,
            container,
            protocol: "tcp".to_string(),
        })
    }
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
        // Test deserialization from YAML string format
        let yaml = r#"
type: docker
image: "ros:humble"
volumes:
  - "/data:/workspace"
privileged: true
network: "host"
"#;
        let config: ExecuteConfig = serde_yaml::from_str(yaml).unwrap();
        match config {
            ExecuteConfig::Docker {
                image,
                volumes,
                privileged,
                network,
                ..
            } => {
                assert_eq!(image, "ros:humble");
                assert_eq!(volumes.len(), 1);
                assert_eq!(volumes[0].host(), &PathBuf::from("/data"));
                assert_eq!(volumes[0].container(), &PathBuf::from("/workspace"));
                assert!(!volumes[0].read_only());
                assert!(privileged);
                assert_eq!(network, Some("host".to_string()));
            }
            _ => panic!("Expected Docker variant"),
        }
    }

    #[test]
    fn test_docker_volume_string_parsing() {
        let yaml = r#"
type: docker
image: nginx:latest
volumes:
  - "/host/path:/container/path"
  - "/data:/app:ro"
"#;
        let config: ExecuteConfig = serde_yaml::from_str(yaml).unwrap();
        match config {
            ExecuteConfig::Docker { volumes, .. } => {
                assert_eq!(volumes.len(), 2);
                assert_eq!(volumes[0].host(), &PathBuf::from("/host/path"));
                assert_eq!(volumes[0].container(), &PathBuf::from("/container/path"));
                assert!(!volumes[0].read_only());
                assert_eq!(volumes[1].host(), &PathBuf::from("/data"));
                assert_eq!(volumes[1].container(), &PathBuf::from("/app"));
                assert!(volumes[1].read_only());
            }
            _ => panic!("Expected Docker variant"),
        }
    }

    #[test]
    fn test_docker_port_string_parsing() {
        let yaml = r#"
type: docker
image: nginx:latest
ports:
  - "8080:80"
  - "9090:9000"
"#;
        let config: ExecuteConfig = serde_yaml::from_str(yaml).unwrap();
        match config {
            ExecuteConfig::Docker { ports, .. } => {
                assert_eq!(ports.len(), 2);
                assert_eq!(ports[0].host, 8080);
                assert_eq!(ports[0].container, 80);
                assert_eq!(ports[0].protocol, "tcp");
                assert_eq!(ports[1].host, 9090);
                assert_eq!(ports[1].container, 9000);
            }
            _ => panic!("Expected Docker variant"),
        }
    }
}

// Configuration file types

use crate::{Dependency, ExecuteConfig, HealthChecker, PolicyConfig};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct KrillConfig {
    pub version: String,
    pub name: String,
    #[serde(default)]
    pub log_dir: Option<PathBuf>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    pub services: HashMap<String, ServiceConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServiceConfig {
    pub execute: ExecuteConfig,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
    #[serde(default)]
    pub critical: bool,
    #[serde(default)]
    pub gpu: bool,
    #[serde(default)]
    pub health_check: Option<HealthChecker>,
    #[serde(default)]
    pub policy: PolicyConfig,
}

impl KrillConfig {
    pub fn from_file(path: &PathBuf) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| ConfigError::FileRead(path.clone(), e.to_string()))?;

        let mut config: KrillConfig =
            serde_yaml::from_str(&content).map_err(|e| ConfigError::Parse(e.to_string()))?;

        // Resolve relative paths against the config file's directory
        if let Some(parent) = path.parent() {
            config.resolve_paths(parent);
        }

        config.validate()?;

        Ok(config)
    }

    /// Resolve relative paths in the config against a base directory
    fn resolve_paths(&mut self, base_dir: &std::path::Path) {
        for service in self.services.values_mut() {
            service.execute.resolve_working_dir(base_dir);
        }
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        // Check version
        if self.version != "1" {
            return Err(ConfigError::UnsupportedVersion(self.version.clone()));
        }

        // Validate workspace name
        if self.name.is_empty() {
            return Err(ConfigError::InvalidWorkspaceName(
                "Workspace name cannot be empty".to_string(),
            ));
        }

        if !self
            .name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        {
            return Err(ConfigError::InvalidWorkspaceName(format!(
                "Workspace name '{}' contains invalid characters",
                self.name
            )));
        }

        // Validate services exist
        if self.services.is_empty() {
            return Err(ConfigError::NoServices);
        }

        // Validate each service
        for (name, service) in &self.services {
            service.validate(name)?;

            // Check that dependencies reference valid services
            for dep in &service.dependencies {
                let dep_name = dep.service_name();
                if !self.services.contains_key(dep_name) {
                    return Err(ConfigError::UnknownDependency {
                        service: name.clone(),
                        dependency: dep_name.to_string(),
                    });
                }
            }

            // Docker type requires Pro version
            if matches!(service.execute, ExecuteConfig::Docker { .. }) {
                return Err(ConfigError::DockerRequiresPro);
            }
        }

        Ok(())
    }
}

impl ServiceConfig {
    fn validate(&self, service_name: &str) -> Result<(), ConfigError> {
        // Validate service name
        if service_name.is_empty() {
            return Err(ConfigError::InvalidServiceName(
                "Service name cannot be empty".to_string(),
            ));
        }

        if !service_name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        {
            return Err(ConfigError::InvalidServiceName(format!(
                "Service name '{}' contains invalid characters",
                service_name
            )));
        }

        // Validate shell commands
        if let ExecuteConfig::Shell {
            command,
            stop_command,
            ..
        } = &self.execute
        {
            crate::validation::validate_shell_command(command)?;
            if let Some(stop_cmd) = stop_command {
                crate::validation::validate_shell_command(stop_cmd)?;
            }
        }

        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Failed to read config file {0}: {1}")]
    FileRead(PathBuf, String),

    #[error("Failed to parse config: {0}")]
    Parse(String),

    #[error("Unsupported config version: {0} (expected '1')")]
    UnsupportedVersion(String),

    #[error("Invalid workspace name: {0}")]
    InvalidWorkspaceName(String),

    #[error("Invalid service name: {0}")]
    InvalidServiceName(String),

    #[error("No services defined in configuration")]
    NoServices,

    #[error("Service '{service}' depends on unknown service '{dependency}'")]
    UnknownDependency { service: String, dependency: String },

    #[error("Docker execution type requires Krill Pro (coming soon)")]
    DockerRequiresPro,

    #[error("Unsafe shell command: {0}")]
    UnsafeShellCommand(String),
}

// Bridge validation error
impl From<crate::validation::ValidationError> for ConfigError {
    fn from(err: crate::validation::ValidationError) -> Self {
        ConfigError::UnsafeShellCommand(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_load_valid_config() {
        let yaml = r#"
version: "1"
name: test-workspace
services:
  service1:
    execute:
      type: pixi
      task: test-task
"#;

        let mut file = NamedTempFile::new().unwrap();
        file.write_all(yaml.as_bytes()).unwrap();

        let config = KrillConfig::from_file(&file.path().to_path_buf()).unwrap();
        assert_eq!(config.name, "test-workspace");
        assert_eq!(config.services.len(), 1);
    }

    #[test]
    fn test_invalid_version() {
        let yaml = r#"
version: "2"
name: test
services:
  service1:
    execute:
      type: pixi
      task: test
"#;

        let mut file = NamedTempFile::new().unwrap();
        file.write_all(yaml.as_bytes()).unwrap();

        let result = KrillConfig::from_file(&file.path().to_path_buf());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unsupported"));
    }

    #[test]
    fn test_unknown_dependency() {
        let yaml = r#"
version: "1"
name: test
services:
  service1:
    execute:
      type: pixi
      task: test
    dependencies:
      - nonexistent
"#;

        let mut file = NamedTempFile::new().unwrap();
        file.write_all(yaml.as_bytes()).unwrap();

        let result = KrillConfig::from_file(&file.path().to_path_buf());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown service"));
    }

    #[test]
    fn test_docker_requires_pro() {
        let yaml = r#"
version: "1"
name: test
services:
  service1:
    execute:
      type: docker
      image: ubuntu:latest
"#;

        let mut file = NamedTempFile::new().unwrap();
        file.write_all(yaml.as_bytes()).unwrap();

        let result = KrillConfig::from_file(&file.path().to_path_buf());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Pro"));
    }

    #[test]
    fn test_unsafe_shell_command() {
        let yaml = r#"
version: "1"
name: test
services:
  service1:
    execute:
      type: shell
      command: "ls | grep foo"
"#;

        let mut file = NamedTempFile::new().unwrap();
        file.write_all(yaml.as_bytes()).unwrap();

        let result = KrillConfig::from_file(&file.path().to_path_buf());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unsafe"));
    }
}

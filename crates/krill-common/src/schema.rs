use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::model::{
    HealthCheck, HealthCheckType, RestartPolicy, RestartPolicyCondition, ServiceConfig,
    ServicesConfig,
};

/// Load and validate service configuration from a YAML file
pub fn load_config_from_file<P: AsRef<Path>>(path: P) -> Result<ServicesConfig, String> {
    let content = fs::read_to_string(&path).map_err(|e| {
        format!(
            "Failed to read config file {}: {}",
            path.as_ref().display(),
            e
        )
    })?;

    load_config_from_str(&content)
}

/// Load and validate service configuration from a YAML string
pub fn load_config_from_str(yaml: &str) -> Result<ServicesConfig, String> {
    let config: ServicesConfig =
        serde_yaml::from_str(yaml).map_err(|e| format!("Failed to parse YAML: {}", e))?;

    // Run basic validation from the model
    config.validate()?;

    // Run extended validation
    let extended_errors = extended_validation(&config);
    if !extended_errors.is_empty() {
        return Err(extended_errors.join("\n"));
    }

    Ok(config)
}

/// Perform extended validation beyond basic structural checks
pub fn extended_validation(config: &ServicesConfig) -> Vec<String> {
    let mut errors = Vec::new();

    for (name, service) in &config.services {
        // Critical services should have health checks
        if service.critical && service.health_check.is_none() {
            errors.push(format!(
                "Critical service '{}' should have a health check defined",
                name
            ));
        }

        // Check health check timeouts
        if let Some(health_check) = &service.health_check {
            match health_check.check_type {
                HealthCheckType::Heartbeat => {
                    // Heartbeat timeouts should be reasonable (>= 1 second)
                    if health_check.timeout_sec < 1 {
                        errors.push(format!(
                            "Service '{}': heartbeat timeout ({}) should be at least 1 second",
                            name, health_check.timeout_sec
                        ));
                    }
                }
                HealthCheckType::Tcp => {
                    if health_check.port.is_none() {
                        errors.push(format!(
                            "Service '{}': TCP health check requires a port",
                            name
                        ));
                    }
                }
                HealthCheckType::Command => {
                    if health_check
                        .command
                        .as_ref()
                        .map(|c| c.trim().is_empty())
                        .unwrap_or(true)
                    {
                        errors.push(format!(
                            "Service '{}': command health check requires a command",
                            name
                        ));
                    }
                }
            }
        }

        // Check restart policy settings
        if let Some(restart_policy) = &service.restart_policy {
            if restart_policy.max_attempts == 0 {
                errors.push(format!("Service '{}': max_attempts must be positive", name));
            }

            // Warn about potentially dangerous configurations
            if service.critical && matches!(restart_policy.condition, RestartPolicyCondition::Never)
            {
                errors.push(format!(
                    "Service '{}': critical service with restart_policy 'never' may leave system in unsafe state",
                    name
                ));
            }
        }

        // Validate command is not empty (already in basic validation, but double-check)
        if service.command.trim().is_empty() {
            errors.push(format!("Service '{}': command cannot be empty", name));
        }
    }

    // Check for dependency cycles (already done in basic validation, but we can add more info)
    // Check for orphaned dependencies (services that nothing depends on but are not marked as entry points)
    // This is just informational, not an error

    errors
}

/// Generate an example configuration for documentation/testing
pub fn example_config() -> ServicesConfig {
    let mut services = HashMap::new();

    services.insert(
        "lidar".to_string(),
        ServiceConfig {
            command: "/usr/bin/lidar_driver --port 9090".to_string(),
            stop_cmd: Some("/usr/bin/lidar_driver --stop".to_string()),
            restart_policy: Some(RestartPolicy {
                condition: RestartPolicyCondition::OnFailure,
                max_attempts: 3,
                delay_sec: 2,
            }),
            critical: true,
            health_check: Some(HealthCheck {
                check_type: HealthCheckType::Heartbeat,
                timeout_sec: 5,
                port: None,
                command: None,
            }),
            dependencies: vec![],
            environment: Some({
                let mut env = HashMap::new();
                env.insert("LOG_LEVEL".to_string(), "INFO".to_string());
                env
            }),
            working_directory: Some("/var/lib/krill".to_string()),
        },
    );

    services.insert(
        "navigator".to_string(),
        ServiceConfig {
            command: "/usr/bin/navigator --config /etc/nav.yaml".to_string(),
            stop_cmd: Some("/usr/bin/navigator --stop".to_string()),
            restart_policy: Some(RestartPolicy {
                condition: RestartPolicyCondition::OnFailure,
                max_attempts: 5,
                delay_sec: 1,
            }),
            critical: false,
            health_check: Some(HealthCheck {
                check_type: HealthCheckType::Heartbeat,
                timeout_sec: 10,
                port: None,
                command: None,
            }),
            dependencies: vec![crate::model::Dependency {
                service: "lidar".to_string(),
                condition: crate::model::DependencyCondition::Healthy,
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

/// Resolve environment variable placeholders in configuration
/// Supports ${VAR_NAME} or $VAR_NAME syntax in command strings
pub fn resolve_environment_variables(config: &mut ServicesConfig) -> Result<(), String> {
    for (_, service) in &mut config.services {
        // Resolve in command
        service.command = resolve_env_in_string(&service.command)?;

        // Resolve in stop command
        if let Some(stop_cmd) = &mut service.stop_cmd {
            *stop_cmd = resolve_env_in_string(stop_cmd)?;
        }

        // Resolve in health check command
        if let Some(health_check) = &mut service.health_check {
            if let Some(cmd) = &mut health_check.command {
                *cmd = resolve_env_in_string(cmd)?;
            }
        }

        // Resolve in working directory
        if let Some(working_dir) = &mut service.working_directory {
            *working_dir = resolve_env_in_string(working_dir)?;
        }

        // Resolve environment variables in environment map values
        if let Some(env_map) = &mut service.environment {
            for value in env_map.values_mut() {
                *value = resolve_env_in_string(value)?;
            }
        }
    }

    Ok(())
}

fn resolve_env_in_string(input: &str) -> Result<String, String> {
    let mut result = input.to_string();

    // Simple implementation: replace ${VAR} or $VAR with env var
    // This could be enhanced with proper parsing
    let mut i = 0;
    while i < result.len() {
        if result[i..].starts_with('$') {
            let start = i;
            i += 1;

            // Check for ${VAR} syntax
            if result[i..].starts_with('{') {
                i += 1;
                let var_start = i;
                while i < result.len() && result.chars().nth(i) != Some('}') {
                    i += 1;
                }
                if i >= result.len() {
                    return Err(format!("Unclosed ${{}} in string: {}", input));
                }
                let var_name = &result[var_start..i];
                i += 1; // Skip '}'

                let var_value = std::env::var(var_name)
                    .map_err(|_| format!("Environment variable '{}' not found", var_name))?;
                result.replace_range(start..i, &var_value);
                i = start + var_value.len();
            } else {
                // $VAR syntax
                let var_start = i;
                while i < result.len()
                    && (result.chars().nth(i).unwrap().is_alphanumeric()
                        || result.chars().nth(i).unwrap() == '_')
                {
                    i += 1;
                }
                if i == var_start {
                    // Just a $ character, leave it alone
                    continue;
                }
                let var_name = &result[var_start..i];
                let var_value = std::env::var(var_name)
                    .map_err(|_| format!("Environment variable '{}' not found", var_name))?;
                result.replace_range(start..i, &var_value);
                i = start + var_value.len();
            }
        } else {
            i += 1;
        }
    }

    Ok(result)
}

/// Generate a JSON schema for the configuration format
/// This can be used for external validation tools
pub fn generate_json_schema() -> serde_json::Value {
    serde_json::json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "title": "Krill Service Configuration",
        "description": "Configuration for robot service orchestration",
        "type": "object",
        "required": ["version", "services"],
        "properties": {
            "version": {
                "type": "string",
                "pattern": "^\\d+(\\.\\d+)*$",
                "description": "Configuration schema version"
            },
            "services": {
                "type": "object",
                "additionalProperties": {
                    "$ref": "#/definitions/ServiceConfig"
                },
                "description": "Map of service names to their configurations"
            }
        },
        "definitions": {
            "ServiceConfig": {
                "type": "object",
                "required": ["command"],
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Command to start the service"
                    },
                    "stop_cmd": {
                        "type": "string",
                        "description": "Command to gracefully stop the service"
                    },
                    "restart_policy": {
                        "$ref": "#/definitions/RestartPolicy"
                    },
                    "critical": {
                        "type": "boolean",
                        "default": false,
                        "description": "Whether this service is critical to system safety"
                    },
                    "health_check": {
                        "$ref": "#/definitions/HealthCheck"
                    },
                    "dependencies": {
                        "type": "array",
                        "items": {
                            "$ref": "#/definitions/Dependency"
                        },
                        "default": [],
                        "description": "Services that must be started/healthy before this service"
                    },
                    "environment": {
                        "type": "object",
                        "additionalProperties": {
                            "type": "string"
                        },
                        "description": "Environment variables to set for the service"
                    },
                    "working_directory": {
                        "type": "string",
                        "description": "Working directory for the service process"
                    }
                }
            },
            "RestartPolicy": {
                "type": "object",
                "required": ["condition"],
                "properties": {
                    "condition": {
                        "type": "string",
                        "enum": ["always", "never", "on-failure"],
                        "description": "When to restart the service"
                    },
                    "max_attempts": {
                        "type": "integer",
                        "minimum": 1,
                        "default": 3,
                        "description": "Maximum restart attempts before giving up"
                    },
                    "delay_sec": {
                        "type": "integer",
                        "minimum": 0,
                        "default": 2,
                        "description": "Delay between restart attempts in seconds"
                    }
                }
            },
            "HealthCheck": {
                "type": "object",
                "required": ["type"],
                "properties": {
                    "type": {
                        "type": "string",
                        "enum": ["heartbeat", "tcp", "command"],
                        "description": "Type of health check to perform"
                    },
                    "timeout_sec": {
                        "type": "integer",
                        "minimum": 1,
                        "default": 5,
                        "description": "Timeout for health check in seconds"
                    },
                    "port": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 65535,
                        "description": "TCP port to check (required for tcp type)"
                    },
                    "command": {
                        "type": "string",
                        "description": "Command to run for health check (required for command type)"
                    }
                }
            },
            "Dependency": {
                "type": "object",
                "required": ["service", "condition"],
                "properties": {
                    "service": {
                        "type": "string",
                        "description": "Name of the service this depends on"
                    },
                    "condition": {
                        "type": "string",
                        "enum": ["started", "healthy"],
                        "description": "Required state of the dependency"
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_config_from_str() {
        let yaml = r#"
version: "1"
services:
  test:
    command: "/usr/bin/test"
    critical: false
"#;

        let result = load_config_from_str(yaml);
        assert!(result.is_ok());
        let config = result.unwrap();
        assert_eq!(config.services.len(), 1);
        assert!(config.services.contains_key("test"));
    }

    #[test]
    fn test_extended_validation() {
        let mut services = HashMap::new();
        services.insert(
            "critical_no_health".to_string(),
            ServiceConfig {
                command: "/usr/bin/test".to_string(),
                stop_cmd: None,
                restart_policy: None,
                critical: true,
                health_check: None,
                dependencies: vec![],
                environment: None,
                working_directory: None,
            },
        );

        let config = ServicesConfig {
            version: "1".to_string(),
            services,
        };

        let errors = extended_validation(&config);
        assert!(!errors.is_empty());
        assert!(errors[0].contains("should have a health check"));
    }

    #[test]
    fn test_resolve_environment_variables() {
        unsafe {
            std::env::set_var("KRILL_HOME", "/opt/krill");
            std::env::set_var("PORT", "9090");
        }

        let mut services = HashMap::new();
        services.insert(
            "test".to_string(),
            ServiceConfig {
                command: "${KRILL_HOME}/bin/test --port $PORT".to_string(),
                stop_cmd: Some("$KRILL_HOME/bin/test --stop".to_string()),
                restart_policy: None,
                critical: false,
                health_check: None,
                dependencies: vec![],
                environment: None,
                working_directory: Some("${KRILL_HOME}/logs".to_string()),
            },
        );

        let mut config = ServicesConfig {
            version: "1".to_string(),
            services,
        };

        let result = resolve_environment_variables(&mut config);
        assert!(result.is_ok());

        let service = config.services.get("test").unwrap();
        assert_eq!(service.command, "/opt/krill/bin/test --port 9090");
        assert_eq!(
            service.stop_cmd.as_ref().unwrap(),
            "/opt/krill/bin/test --stop"
        );
        assert_eq!(
            service.working_directory.as_ref().unwrap(),
            "/opt/krill/logs"
        );

        // Clean up
        unsafe {
            std::env::remove_var("KRILL_HOME");
            std::env::remove_var("PORT");
        }
    }

    #[test]
    fn test_example_config() {
        let config = example_config();
        assert_eq!(config.services.len(), 2);
        assert!(config.services.contains_key("lidar"));
        assert!(config.services.contains_key("navigator"));

        let lidar = config.services.get("lidar").unwrap();
        assert!(lidar.critical);
        assert!(lidar.health_check.is_some());
    }

    #[test]
    fn test_generate_json_schema() {
        let schema = generate_json_schema();
        assert_eq!(schema["title"], "Krill Service Configuration");
        assert_eq!(schema["type"], "object");
    }
}

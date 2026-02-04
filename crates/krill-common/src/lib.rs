pub mod model;
pub mod schema;

// Re-export commonly used types for convenience
pub use model::{
    Dependency, DependencyCondition, EventMessage, EventType, HealthCheck, HealthCheckType,
    HeartbeatMessage, Message, RequestMessage, ResponseError, ResponseMessage, RestartPolicy,
    RestartPolicyCondition, ServiceConfig, ServiceState, ServiceStatus, ServicesConfig,
};

/// Version of the common library API
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;
    use serde_yaml;

    #[test]
    fn test_service_config_serialization() {
        let config = ServiceConfig {
            command: "/usr/bin/test".to_string(),
            stop_cmd: Some("/usr/bin/test --stop".to_string()),
            restart_policy: Some(RestartPolicy {
                condition: RestartPolicyCondition::OnFailure,
                max_attempts: 3,
                delay_sec: 2,
            }),
            critical: false,
            health_check: Some(HealthCheck {
                check_type: HealthCheckType::Heartbeat,
                timeout_sec: 5,
                port: None,
                command: None,
            }),
            dependencies: vec![Dependency {
                service: "dep_service".to_string(),
                condition: DependencyCondition::Healthy,
            }],
            environment: None,
            working_directory: None,
        };

        let yaml = serde_yaml::to_string(&config).unwrap();
        let parsed: ServiceConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(config.command, parsed.command);
    }

    #[test]
    fn test_services_config_validation() {
        let mut services = std::collections::HashMap::new();
        services.insert(
            "test".to_string(),
            ServiceConfig {
                command: "/usr/bin/test".to_string(),
                stop_cmd: None,
                restart_policy: None,
                critical: false,
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

        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_circular_dependency_detection() {
        let mut services = std::collections::HashMap::new();

        // Service A depends on B
        services.insert(
            "service_a".to_string(),
            ServiceConfig {
                command: "/usr/bin/a".to_string(),
                stop_cmd: None,
                restart_policy: None,
                critical: false,
                health_check: None,
                dependencies: vec![Dependency {
                    service: "service_b".to_string(),
                    condition: DependencyCondition::Healthy,
                }],
                environment: None,
                working_directory: None,
            },
        );

        // Service B depends on A (circular)
        services.insert(
            "service_b".to_string(),
            ServiceConfig {
                command: "/usr/bin/b".to_string(),
                stop_cmd: None,
                restart_policy: None,
                critical: false,
                health_check: None,
                dependencies: vec![Dependency {
                    service: "service_a".to_string(),
                    condition: DependencyCondition::Healthy,
                }],
                environment: None,
                working_directory: None,
            },
        );

        let config = ServicesConfig {
            version: "1".to_string(),
            services,
        };

        assert!(config.validate().is_err());
        let err = config.validate().unwrap_err();
        assert!(err.to_lowercase().contains("circular"));
    }
}

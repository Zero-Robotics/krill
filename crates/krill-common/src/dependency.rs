// Phase 1.2: Dependencies (Simple + Condition Syntax)

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Dependency {
    // "service-name" => depends on service being started
    Simple(String),
    // "service-name healthy" => depends on service being healthy
    WithCondition {
        service: String,
        condition: DependencyCondition,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DependencyCondition {
    Started,
    Healthy,
}

impl Dependency {
    /// Get the service name this dependency refers to
    pub fn service_name(&self) -> &str {
        match self {
            Dependency::Simple(name) => name,
            Dependency::WithCondition { service, .. } => service,
        }
    }

    /// Get the condition required for this dependency (defaults to Started)
    pub fn condition(&self) -> DependencyCondition {
        match self {
            Dependency::Simple(_) => DependencyCondition::Started,
            Dependency::WithCondition { condition, .. } => *condition,
        }
    }
}

// Custom deserialization to support "service healthy" string syntax
impl<'de> Deserialize<'de> for Dependency {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;

        let s = String::deserialize(deserializer)?;
        let parts: Vec<&str> = s.split_whitespace().collect();

        match parts.as_slice() {
            [service] => Ok(Dependency::Simple(service.to_string())),
            [service, "started"] => Ok(Dependency::WithCondition {
                service: service.to_string(),
                condition: DependencyCondition::Started,
            }),
            [service, "healthy"] => Ok(Dependency::WithCondition {
                service: service.to_string(),
                condition: DependencyCondition::Healthy,
            }),
            _ => Err(D::Error::custom(format!(
                "Invalid dependency format: '{}'. Expected 'service' or 'service condition'",
                s
            ))),
        }
    }
}

impl Serialize for Dependency {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let s = match self {
            Dependency::Simple(name) => name.clone(),
            Dependency::WithCondition { service, condition } => {
                format!(
                    "{} {}",
                    service,
                    match condition {
                        DependencyCondition::Started => "started",
                        DependencyCondition::Healthy => "healthy",
                    }
                )
            }
        };
        serializer.serialize_str(&s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_dependency() {
        let dep = Dependency::Simple("lidar".to_string());
        assert_eq!(dep.service_name(), "lidar");
        assert_eq!(dep.condition(), DependencyCondition::Started);
    }

    #[test]
    fn test_dependency_with_condition() {
        let dep = Dependency::WithCondition {
            service: "lidar".to_string(),
            condition: DependencyCondition::Healthy,
        };
        assert_eq!(dep.service_name(), "lidar");
        assert_eq!(dep.condition(), DependencyCondition::Healthy);
    }

    #[test]
    fn test_deserialize_simple() {
        let yaml = r#""lidar""#;
        let dep: Dependency = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(dep.service_name(), "lidar");
        assert_eq!(dep.condition(), DependencyCondition::Started);
    }

    #[test]
    fn test_deserialize_with_healthy() {
        let yaml = r#""lidar healthy""#;
        let dep: Dependency = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(dep.service_name(), "lidar");
        assert_eq!(dep.condition(), DependencyCondition::Healthy);
    }

    #[test]
    fn test_serialize_simple() {
        let dep = Dependency::Simple("lidar".to_string());
        let yaml = serde_yaml::to_string(&dep).unwrap();
        assert_eq!(yaml.trim(), "lidar");
    }

    #[test]
    fn test_serialize_with_condition() {
        let dep = Dependency::WithCondition {
            service: "lidar".to_string(),
            condition: DependencyCondition::Healthy,
        };
        let yaml = serde_yaml::to_string(&dep).unwrap();
        assert_eq!(yaml.trim(), "lidar healthy");
    }
}

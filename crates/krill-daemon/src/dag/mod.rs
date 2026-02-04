use std::collections::{HashMap, HashSet, VecDeque};

use krill_common::model::{Dependency, DependencyCondition, ServicesConfig};

/// Dependency graph for service orchestration
#[derive(Debug, Clone)]
pub struct DependencyGraph {
    /// Forward dependencies: service -> list of services that depend on it
    forward: HashMap<String, HashSet<String>>,
    /// Reverse dependencies: service -> list of services it depends on (with conditions)
    reverse: HashMap<String, Vec<Dependency>>,
    /// All services in the graph
    services: HashSet<String>,
}

/// Result of a topological sort
pub struct TopologicalOrder {
    /// Services in topological order
    pub order: Vec<String>,
    /// Services that could not be sorted (if cycles exist)
    pub unsortable: Vec<String>,
}

impl DependencyGraph {
    /// Create a new dependency graph from service configuration
    pub fn new(config: &ServicesConfig) -> Result<Self, String> {
        let mut forward: HashMap<String, HashSet<String>> = HashMap::new();
        let mut reverse: HashMap<String, Vec<Dependency>> = HashMap::new();
        let mut services = HashSet::new();

        // Initialize structures for all services
        for service_name in config.services.keys() {
            services.insert(service_name.clone());
            forward.insert(service_name.clone(), HashSet::new());
            reverse.insert(service_name.clone(), Vec::new());
        }

        // Build dependency relationships
        for (service_name, service_config) in &config.services {
            for dependency in &service_config.dependencies {
                // Check if dependency exists
                if !config.services.contains_key(&dependency.service) {
                    return Err(format!(
                        "Service '{}' depends on unknown service '{}'",
                        service_name, dependency.service
                    ));
                }

                // Add to forward dependencies (who depends on me)
                forward
                    .entry(dependency.service.clone())
                    .or_default()
                    .insert(service_name.clone());

                // Add to reverse dependencies (who I depend on)
                reverse
                    .entry(service_name.clone())
                    .or_default()
                    .push(dependency.clone());
            }
        }

        let graph = Self {
            forward,
            reverse,
            services,
        };

        // Validate no cycles
        if graph.has_cycles() {
            return Err("Circular dependency detected in service configuration".to_string());
        }

        Ok(graph)
    }

    /// Check if the graph contains cycles
    pub fn has_cycles(&self) -> bool {
        let mut visited = HashSet::new();
        let mut recursion_stack = HashSet::new();

        for service in &self.services {
            if !visited.contains(service) {
                if self.has_cycles_dfs(service, &mut visited, &mut recursion_stack) {
                    return true;
                }
            }
        }

        false
    }

    /// DFS helper for cycle detection
    fn has_cycles_dfs(
        &self,
        service: &str,
        visited: &mut HashSet<String>,
        recursion_stack: &mut HashSet<String>,
    ) -> bool {
        visited.insert(service.to_string());
        recursion_stack.insert(service.to_string());

        // Traverse dependencies (reverse edges) to find cycles
        if let Some(dependencies) = self.reverse.get(service) {
            for dependency in dependencies {
                let dep_service = &dependency.service;
                if !visited.contains(dep_service.as_str()) {
                    if self.has_cycles_dfs(dep_service, visited, recursion_stack) {
                        return true;
                    }
                } else if recursion_stack.contains(dep_service.as_str()) {
                    return true;
                }
            }
        }

        recursion_stack.remove(service);
        false
    }

    /// Get topological order for service startup
    /// Uses Kahn's algorithm
    pub fn topological_order(&self) -> TopologicalOrder {
        let mut in_degree = HashMap::new();
        let mut order = Vec::new();
        let mut queue = VecDeque::new();

        // Calculate in-degrees
        for service in &self.services {
            let degree = self
                .reverse
                .get(service)
                .map(|deps| deps.len())
                .unwrap_or(0);
            in_degree.insert(service.clone(), degree);

            if degree == 0 {
                queue.push_back(service.clone());
            }
        }

        // Process queue
        while let Some(service) = queue.pop_front() {
            order.push(service.clone());

            if let Some(dependents) = self.forward.get(&service) {
                for dependent in dependents {
                    let degree = in_degree
                        .get_mut(dependent)
                        .expect("Service should have in-degree");
                    *degree -= 1;

                    if *degree == 0 {
                        queue.push_back(dependent.clone());
                    }
                }
            }
        }

        // Check for unsortable services (cycles, though we validate at construction)
        let unsortable: Vec<String> = in_degree
            .into_iter()
            .filter(|(_, degree)| *degree > 0)
            .map(|(service, _)| service)
            .collect();

        TopologicalOrder { order, unsortable }
    }

    /// Get reverse topological order for service shutdown
    /// This ensures dependencies are stopped after dependents
    pub fn reverse_topological_order(&self) -> Vec<String> {
        let mut order = self.topological_order().order;
        order.reverse();
        order
    }

    /// Get all services that depend on a given service
    pub fn get_dependents(&self, service: &str) -> Vec<String> {
        self.forward
            .get(service)
            .map(|deps| deps.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Get all direct dependencies for a service
    pub fn get_dependencies(&self, service: &str) -> Vec<Dependency> {
        self.reverse
            .get(service)
            .map(|deps| deps.clone())
            .unwrap_or_default()
    }

    /// Get all transitive dependencies (recursive) for a service
    pub fn get_transitive_dependencies(&self, service: &str) -> HashSet<String> {
        let mut dependencies = HashSet::new();
        let mut stack = vec![service.to_string()];

        while let Some(current) = stack.pop() {
            if let Some(deps) = self.reverse.get(&current) {
                for dep in deps {
                    if !dependencies.contains(&dep.service) {
                        dependencies.insert(dep.service.clone());
                        stack.push(dep.service.clone());
                    }
                }
            }
        }

        dependencies
    }

    /// Get all transitive dependents (recursive) for a service
    pub fn get_transitive_dependents(&self, service: &str) -> HashSet<String> {
        let mut dependents = HashSet::new();
        let mut stack = vec![service.to_string()];

        while let Some(current) = stack.pop() {
            if let Some(deps) = self.forward.get(&current) {
                for dep in deps {
                    if !dependents.contains(dep) {
                        dependents.insert(dep.clone());
                        stack.push(dep.clone());
                    }
                }
            }
        }

        dependents
    }

    /// Check if all dependencies for a service are satisfied
    /// `get_state` is a function that returns the current state of a service
    pub fn are_dependencies_satisfied<F>(&self, service: &str, get_state: F) -> bool
    where
        F: Fn(&str) -> krill_common::model::ServiceState,
    {
        let Some(dependencies) = self.reverse.get(service) else {
            return true; // No dependencies
        };

        for dependency in dependencies {
            let state = get_state(&dependency.service);
            match dependency.condition {
                DependencyCondition::Started => {
                    // Service must be Running or Healthy (not Stopping - that would cause race conditions)
                    if !matches!(
                        state,
                        krill_common::model::ServiceState::Running
                            | krill_common::model::ServiceState::Healthy
                    ) {
                        return false;
                    }
                }
                DependencyCondition::Healthy => {
                    // Service must be Healthy
                    if !matches!(state, krill_common::model::ServiceState::Healthy) {
                        return false;
                    }
                }
            }
        }

        true
    }

    /// Get services that are ready to start (dependencies satisfied)
    pub fn get_ready_services<F>(&self, get_state: F) -> Vec<String>
    where
        F: Fn(&str) -> krill_common::model::ServiceState,
    {
        let mut ready = Vec::new();

        for service in &self.services {
            if self.are_dependencies_satisfied(service, &get_state) {
                ready.push(service.clone());
            }
        }

        ready
    }

    /// Get all services in the graph
    pub fn all_services(&self) -> Vec<String> {
        self.services.iter().cloned().collect()
    }

    /// Check if a service exists in the graph
    pub fn contains_service(&self, service: &str) -> bool {
        self.services.contains(service)
    }

    /// Find services with no dependencies (entry points)
    pub fn entry_points(&self) -> Vec<String> {
        self.services
            .iter()
            .filter(|service| {
                self.reverse
                    .get(*service)
                    .map(|deps| deps.is_empty())
                    .unwrap_or(true)
            })
            .cloned()
            .collect()
    }

    /// Find services that nothing depends on (exit points)
    pub fn exit_points(&self) -> Vec<String> {
        self.services
            .iter()
            .filter(|service| {
                self.forward
                    .get(*service)
                    .map(|deps| deps.is_empty())
                    .unwrap_or(true)
            })
            .cloned()
            .collect()
    }
}

impl TopologicalOrder {
    /// Check if the topological sort was successful (no cycles)
    pub fn is_valid(&self) -> bool {
        self.unsortable.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use krill_common::model::{ServiceConfig, ServiceState};

    fn create_test_config() -> ServicesConfig {
        let mut services = HashMap::new();

        // Service A: no dependencies
        services.insert(
            "service_a".to_string(),
            ServiceConfig {
                command: "/usr/bin/a".to_string(),
                stop_cmd: None,
                restart_policy: None,
                critical: false,
                health_check: None,
                dependencies: vec![],
                environment: None,
                working_directory: None,
            },
        );

        // Service B: depends on A (healthy)
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

        // Service C: depends on A (started) and B (healthy)
        services.insert(
            "service_c".to_string(),
            ServiceConfig {
                command: "/usr/bin/c".to_string(),
                stop_cmd: None,
                restart_policy: None,
                critical: false,
                health_check: None,
                dependencies: vec![
                    Dependency {
                        service: "service_a".to_string(),
                        condition: DependencyCondition::Started,
                    },
                    Dependency {
                        service: "service_b".to_string(),
                        condition: DependencyCondition::Healthy,
                    },
                ],
                environment: None,
                working_directory: None,
            },
        );

        ServicesConfig {
            version: "1".to_string(),
            services,
        }
    }

    #[test]
    fn test_graph_creation() {
        let config = create_test_config();
        let graph = DependencyGraph::new(&config).unwrap();

        assert_eq!(graph.all_services().len(), 3);
        assert!(graph.contains_service("service_a"));
        assert!(graph.contains_service("service_b"));
        assert!(graph.contains_service("service_c"));
    }

    #[test]
    fn test_cycle_detection() {
        let mut config = create_test_config();

        // Add circular dependency
        if let Some(service_a) = config.services.get_mut("service_a") {
            service_a.dependencies.push(Dependency {
                service: "service_c".to_string(),
                condition: DependencyCondition::Started,
            });
        }

        let graph_result = DependencyGraph::new(&config);
        assert!(graph_result.is_err());
        assert!(
            graph_result
                .unwrap_err()
                .to_lowercase()
                .contains("circular")
        );
    }

    #[test]
    fn test_topological_order() {
        let config = create_test_config();
        let graph = DependencyGraph::new(&config).unwrap();
        let order = graph.topological_order();

        assert!(order.is_valid());

        // A should come before B and C
        let a_pos = order.order.iter().position(|s| s == "service_a").unwrap();
        let b_pos = order.order.iter().position(|s| s == "service_b").unwrap();
        let c_pos = order.order.iter().position(|s| s == "service_c").unwrap();

        assert!(a_pos < b_pos);
        assert!(b_pos < c_pos);
    }

    #[test]
    fn test_reverse_topological_order() {
        let config = create_test_config();
        let graph = DependencyGraph::new(&config).unwrap();
        let order = graph.reverse_topological_order();

        // C should come before B and A (for shutdown)
        let a_pos = order.iter().position(|s| s == "service_a").unwrap();
        let b_pos = order.iter().position(|s| s == "service_b").unwrap();
        let c_pos = order.iter().position(|s| s == "service_c").unwrap();

        assert!(c_pos < b_pos);
        assert!(b_pos < a_pos);
    }

    #[test]
    fn test_dependency_checking() {
        let config = create_test_config();
        let graph = DependencyGraph::new(&config).unwrap();

        // Mock state function
        let get_state = |service: &str| match service {
            "service_a" => ServiceState::Running,
            "service_b" => ServiceState::Stopped,
            "service_c" => ServiceState::Stopped,
            _ => ServiceState::Stopped,
        };

        // Service A has no dependencies, so it's ready
        assert!(graph.are_dependencies_satisfied("service_a", &get_state));

        // Service B depends on A being Healthy, but A is only Running
        assert!(!graph.are_dependencies_satisfied("service_b", &get_state));

        // Change A to Healthy
        let get_state_healthy = |service: &str| match service {
            "service_a" => ServiceState::Healthy,
            "service_b" => ServiceState::Stopped,
            "service_c" => ServiceState::Stopped,
            _ => ServiceState::Stopped,
        };

        assert!(graph.are_dependencies_satisfied("service_b", &get_state_healthy));
    }

    #[test]
    fn test_get_dependents() {
        let config = create_test_config();
        let graph = DependencyGraph::new(&config).unwrap();

        let dependents = graph.get_dependents("service_a");
        assert_eq!(dependents.len(), 2);
        assert!(dependents.contains(&"service_b".to_string()));
        assert!(dependents.contains(&"service_c".to_string()));

        let dependents = graph.get_dependents("service_b");
        assert_eq!(dependents.len(), 1);
        assert!(dependents.contains(&"service_c".to_string()));

        let dependents = graph.get_dependents("service_c");
        assert_eq!(dependents.len(), 0);
    }

    #[test]
    fn test_transitive_dependents() {
        let config = create_test_config();
        let graph = DependencyGraph::new(&config).unwrap();

        let transitive = graph.get_transitive_dependents("service_a");
        assert_eq!(transitive.len(), 2);
        assert!(transitive.contains(&"service_b".to_string()));
        assert!(transitive.contains(&"service_c".to_string()));
    }
}

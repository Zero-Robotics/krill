use crate::dependency::{Dependency, DependencyCondition};
use crate::ipc::ServiceStatus;
use std::collections::{HashMap, HashSet, VecDeque};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DagError {
    #[error("Circular dependency detected involving: {0}")]
    CircularDependency(String),

    #[error("Unknown service: {0}")]
    UnknownService(String),

    #[error("Cannot determine order: {0}")]
    OrderingError(String),
}

pub struct DependencyGraph {
    /// Forward edges: service -> services that depend on it
    edges: HashMap<String, HashSet<String>>,

    /// Reverse edges: service -> services it depends on
    reverse_edges: HashMap<String, Vec<Dependency>>,

    /// All services in the graph
    services: HashSet<String>,
}

impl DependencyGraph {
    /// Create a new dependency graph from service definitions
    pub fn new(services: &HashMap<String, Vec<Dependency>>) -> Result<Self, DagError> {
        let mut edges: HashMap<String, HashSet<String>> = HashMap::new();
        let mut reverse_edges: HashMap<String, Vec<Dependency>> = HashMap::new();
        let mut all_services = HashSet::new();

        // Collect all service names first
        for service_name in services.keys() {
            all_services.insert(service_name.clone());
            edges.insert(service_name.clone(), HashSet::new());
            reverse_edges.insert(service_name.clone(), Vec::new());
        }

        // Build dependency edges
        for (service_name, dependencies) in services {
            for dep in dependencies {
                let dep_service = dep.service_name();

                // Validate that the dependency exists
                if !all_services.contains(dep_service) {
                    return Err(DagError::UnknownService(dep_service.to_string()));
                }

                // Add forward edge: dep_service -> service_name
                edges
                    .get_mut(dep_service)
                    .unwrap()
                    .insert(service_name.clone());

                // Add reverse edge: service_name -> dep_service
                reverse_edges
                    .get_mut(service_name)
                    .unwrap()
                    .push(dep.clone());
            }
        }

        let graph = Self {
            edges,
            reverse_edges,
            services: all_services,
        };

        // Validate no cycles
        graph.check_cycles()?;

        Ok(graph)
    }

    /// Check for circular dependencies using DFS
    fn check_cycles(&self) -> Result<(), DagError> {
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();

        for service in &self.services {
            if !visited.contains(service) {
                if let Some(cycle) = self.dfs_cycle_check(service, &mut visited, &mut rec_stack) {
                    return Err(DagError::CircularDependency(cycle));
                }
            }
        }

        Ok(())
    }

    fn dfs_cycle_check(
        &self,
        service: &str,
        visited: &mut HashSet<String>,
        rec_stack: &mut HashSet<String>,
    ) -> Option<String> {
        visited.insert(service.to_string());
        rec_stack.insert(service.to_string());

        // Follow dependencies (reverse edges)
        if let Some(deps) = self.reverse_edges.get(service) {
            for dep in deps {
                let dep_service = dep.service_name();
                if !visited.contains(dep_service) {
                    if let Some(cycle) = self.dfs_cycle_check(dep_service, visited, rec_stack) {
                        return Some(cycle);
                    }
                } else if rec_stack.contains(dep_service) {
                    return Some(format!("{} -> {}", service, dep_service));
                }
            }
        }

        rec_stack.remove(service);
        None
    }

    /// Get startup order using topological sort
    pub fn startup_order(&self) -> Result<Vec<String>, DagError> {
        let mut in_degree: HashMap<String, usize> = HashMap::new();
        let mut queue = VecDeque::new();
        let mut order = Vec::new();

        // Calculate in-degrees
        for service in &self.services {
            let degree = self.reverse_edges.get(service).map_or(0, |deps| deps.len());
            in_degree.insert(service.clone(), degree);

            if degree == 0 {
                queue.push_back(service.clone());
            }
        }

        // Process queue
        while let Some(service) = queue.pop_front() {
            order.push(service.clone());

            // Reduce in-degree for dependent services
            if let Some(dependents) = self.edges.get(&service) {
                for dependent in dependents {
                    let degree = in_degree.get_mut(dependent).unwrap();
                    *degree -= 1;
                    if *degree == 0 {
                        queue.push_back(dependent.clone());
                    }
                }
            }
        }

        if order.len() != self.services.len() {
            return Err(DagError::OrderingError(
                "Unable to determine complete startup order".to_string(),
            ));
        }

        Ok(order)
    }

    /// Get shutdown order (reverse of startup order)
    pub fn shutdown_order(&self) -> Result<Vec<String>, DagError> {
        let mut order = self.startup_order()?;
        order.reverse();
        Ok(order)
    }

    /// Get services that should be stopped when a service fails (cascade failure)
    pub fn cascade_failure(&self, failed_service: &str) -> HashSet<String> {
        let mut to_stop = HashSet::new();
        let mut queue = VecDeque::new();

        queue.push_back(failed_service.to_string());

        while let Some(service) = queue.pop_front() {
            if let Some(dependents) = self.edges.get(&service) {
                for dependent in dependents {
                    if !to_stop.contains(dependent) {
                        to_stop.insert(dependent.clone());
                        queue.push_back(dependent.clone());
                    }
                }
            }
        }

        to_stop
    }

    /// Check if dependencies are satisfied for a service
    pub fn dependencies_satisfied<F>(&self, service: &str, get_status: F) -> bool
    where
        F: Fn(&str) -> ServiceStatus,
    {
        if let Some(deps) = self.reverse_edges.get(service) {
            for dep in deps {
                let dep_service = dep.service_name();
                let status = get_status(dep_service);

                match dep.condition() {
                    DependencyCondition::Started => {
                        if !matches!(
                            status,
                            ServiceStatus::Running
                                | ServiceStatus::Healthy
                                | ServiceStatus::Degraded
                        ) {
                            return false;
                        }
                    }
                    DependencyCondition::Healthy => {
                        if status != ServiceStatus::Healthy {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simple_dep(name: &str) -> Dependency {
        Dependency::Simple(name.to_string())
    }

    fn healthy_dep(name: &str) -> Dependency {
        Dependency::WithCondition {
            service: name.to_string(),
            condition: DependencyCondition::Healthy,
        }
    }

    #[test]
    fn test_simple_graph() {
        let mut services: HashMap<String, Vec<Dependency>> = HashMap::new();
        services.insert("a".to_string(), vec![]);
        services.insert("b".to_string(), vec![simple_dep("a")]);
        services.insert("c".to_string(), vec![simple_dep("b")]);

        let graph = DependencyGraph::new(&services).unwrap();
        let order = graph.startup_order().unwrap();

        assert_eq!(order, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_circular_dependency() {
        let mut services: HashMap<String, Vec<Dependency>> = HashMap::new();
        services.insert("a".to_string(), vec![simple_dep("b")]);
        services.insert("b".to_string(), vec![simple_dep("a")]);

        let result = DependencyGraph::new(&services);
        assert!(matches!(result, Err(DagError::CircularDependency(_))));
    }

    #[test]
    fn test_shutdown_order() {
        let mut services: HashMap<String, Vec<Dependency>> = HashMap::new();
        services.insert("a".to_string(), vec![]);
        services.insert("b".to_string(), vec![simple_dep("a")]);
        services.insert("c".to_string(), vec![simple_dep("b")]);

        let graph = DependencyGraph::new(&services).unwrap();
        let order = graph.shutdown_order().unwrap();

        assert_eq!(order, vec!["c", "b", "a"]);
    }

    #[test]
    fn test_cascade_failure() {
        let mut services: HashMap<String, Vec<Dependency>> = HashMap::new();
        services.insert("a".to_string(), vec![]);
        services.insert("b".to_string(), vec![simple_dep("a")]);
        services.insert("c".to_string(), vec![simple_dep("b")]);
        services.insert("d".to_string(), vec![simple_dep("a")]);

        let graph = DependencyGraph::new(&services).unwrap();
        let affected = graph.cascade_failure("a");

        assert!(affected.contains("b"));
        assert!(affected.contains("c"));
        assert!(affected.contains("d"));
        assert!(!affected.contains("a"));
    }

    #[test]
    fn test_dependencies_satisfied() {
        let mut services: HashMap<String, Vec<Dependency>> = HashMap::new();
        services.insert("a".to_string(), vec![]);
        services.insert("b".to_string(), vec![healthy_dep("a")]);

        let graph = DependencyGraph::new(&services).unwrap();

        // When 'a' is healthy, 'b' dependencies are satisfied
        let satisfied = graph.dependencies_satisfied("b", |name| {
            if name == "a" {
                ServiceStatus::Healthy
            } else {
                ServiceStatus::Stopped
            }
        });
        assert!(satisfied);

        // When 'a' is only running (not healthy), 'b' dependencies are not satisfied
        let not_satisfied = graph.dependencies_satisfied("b", |name| {
            if name == "a" {
                ServiceStatus::Running
            } else {
                ServiceStatus::Stopped
            }
        });
        assert!(!not_satisfied);
    }

    #[test]
    fn test_unknown_service_dependency() {
        let mut services: HashMap<String, Vec<Dependency>> = HashMap::new();
        services.insert("a".to_string(), vec![simple_dep("unknown")]);

        let result = DependencyGraph::new(&services);
        assert!(matches!(result, Err(DagError::UnknownService(_))));
    }
}

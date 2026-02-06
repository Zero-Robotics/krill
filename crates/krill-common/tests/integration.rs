// Integration-style tests for krill-common covering edge cases and
// cross-module interactions not exercised by the inline unit tests.

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};
use tempfile::NamedTempFile;

use krill_common::config::KrillConfig;
use krill_common::dag::DependencyGraph;
use krill_common::dependency::{Dependency, DependencyCondition};
use krill_common::execute::ExecuteConfig;
use krill_common::health::HealthChecker;
use krill_common::ipc::{ClientMessage, CommandAction, LogLevel, ServerMessage, ServiceStatus};
use krill_common::policy::{PolicyConfig, RestartPolicy};
use krill_common::process::{
    build_command, generate_process_name, get_stop_command, get_working_dir,
};
use krill_common::validation::validate_shell_command;

// ---------------------------------------------------------------------------
// config module
// ---------------------------------------------------------------------------
mod config {
    use super::*;

    #[test]
    fn empty_workspace_name_is_rejected() {
        let yaml = r#"
version: "1"
name: ""
services:
  svc:
    execute:
      type: pixi
      task: run
"#;
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(yaml.as_bytes()).unwrap();

        let err = KrillConfig::from_file(&f.path().to_path_buf()).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("empty") || msg.contains("Invalid workspace name"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn invalid_workspace_name_characters() {
        let yaml = r#"
version: "1"
name: "bad name!"
services:
  svc:
    execute:
      type: pixi
      task: run
"#;
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(yaml.as_bytes()).unwrap();

        let err = KrillConfig::from_file(&f.path().to_path_buf()).unwrap_err();
        assert!(
            err.to_string().contains("invalid characters"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn empty_services_map_is_rejected() {
        let yaml = r#"
version: "1"
name: ws
services: {}
"#;
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(yaml.as_bytes()).unwrap();

        let err = KrillConfig::from_file(&f.path().to_path_buf()).unwrap_err();
        assert!(
            err.to_string().contains("No services"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn config_with_env_vars() {
        let yaml = r#"
version: "1"
name: ws
env:
  ROS_DOMAIN_ID: "42"
  LANG: en_US
services:
  svc:
    execute:
      type: pixi
      task: run
"#;
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(yaml.as_bytes()).unwrap();

        let cfg = KrillConfig::from_file(&f.path().to_path_buf()).unwrap();
        assert_eq!(cfg.env.get("ROS_DOMAIN_ID").unwrap(), "42");
        assert_eq!(cfg.env.get("LANG").unwrap(), "en_US");
    }

    #[test]
    fn config_with_log_dir() {
        let yaml = r#"
version: "1"
name: ws
log_dir: /tmp/krill-logs
services:
  svc:
    execute:
      type: pixi
      task: run
"#;
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(yaml.as_bytes()).unwrap();

        let cfg = KrillConfig::from_file(&f.path().to_path_buf()).unwrap();
        assert_eq!(cfg.log_dir, Some(PathBuf::from("/tmp/krill-logs")));
    }

    #[test]
    fn service_name_validation_invalid_chars() {
        // Service name with a space -- serde_yaml will accept the key, then
        // validate() should reject it.
        let yaml = r#"
version: "1"
name: ws
services:
  "bad name":
    execute:
      type: pixi
      task: run
"#;
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(yaml.as_bytes()).unwrap();

        let err = KrillConfig::from_file(&f.path().to_path_buf()).unwrap_err();
        assert!(
            err.to_string().contains("invalid characters") || err.to_string().contains("Invalid"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn config_with_all_execute_types_except_docker() {
        // Test pixi, ros2, and shell execute types together.
        let yaml = r#"
version: "1"
name: multi
services:
  pixi-svc:
    execute:
      type: pixi
      task: start
  ros-svc:
    execute:
      type: ros2
      package: nav2
      launch_file: nav.launch.py
  shell-svc:
    execute:
      type: shell
      command: "python run.py"
"#;
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(yaml.as_bytes()).unwrap();

        let cfg = KrillConfig::from_file(&f.path().to_path_buf()).unwrap();
        assert_eq!(cfg.services.len(), 3);
        assert_eq!(cfg.services["pixi-svc"].execute.executor_type(), "pixi");
        assert_eq!(cfg.services["ros-svc"].execute.executor_type(), "ros2");
        assert_eq!(cfg.services["shell-svc"].execute.executor_type(), "shell");
    }

    #[test]
    fn multiple_services_with_valid_deps() {
        let yaml = r#"
version: "1"
name: ws
services:
  base:
    execute:
      type: pixi
      task: run
  mid:
    execute:
      type: pixi
      task: run
    dependencies:
      - base
  top:
    execute:
      type: pixi
      task: run
    dependencies:
      - mid
"#;
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(yaml.as_bytes()).unwrap();

        let cfg = KrillConfig::from_file(&f.path().to_path_buf()).unwrap();
        assert_eq!(cfg.services.len(), 3);
        assert_eq!(cfg.services["top"].dependencies.len(), 1);
    }
}

// ---------------------------------------------------------------------------
// dag module
// ---------------------------------------------------------------------------
mod dag {
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

    fn started_dep(name: &str) -> Dependency {
        Dependency::WithCondition {
            service: name.to_string(),
            condition: DependencyCondition::Started,
        }
    }

    #[test]
    fn diamond_dependency_graph() {
        // A -> B, A -> C, B -> D, C -> D
        let mut services: HashMap<String, Vec<Dependency>> = HashMap::new();
        services.insert("D".into(), vec![]);
        services.insert("B".into(), vec![simple_dep("D")]);
        services.insert("C".into(), vec![simple_dep("D")]);
        services.insert("A".into(), vec![simple_dep("B"), simple_dep("C")]);

        let graph = DependencyGraph::new(&services).unwrap();
        let order = graph.startup_order().unwrap();

        // D must come before B and C; B and C must come before A
        let pos = |s: &str| order.iter().position(|x| x == s).unwrap();
        assert!(pos("D") < pos("B"));
        assert!(pos("D") < pos("C"));
        assert!(pos("B") < pos("A"));
        assert!(pos("C") < pos("A"));
    }

    #[test]
    fn large_graph_ordering() {
        // Build a linear chain of 50 services: s0 -> s1 -> ... -> s49
        let mut services: HashMap<String, Vec<Dependency>> = HashMap::new();
        services.insert("s0".into(), vec![]);
        for i in 1..50 {
            services.insert(format!("s{i}"), vec![simple_dep(&format!("s{}", i - 1))]);
        }

        let graph = DependencyGraph::new(&services).unwrap();
        let order = graph.startup_order().unwrap();
        assert_eq!(order.len(), 50);

        // Each service should appear after its dependency
        for i in 1..50 {
            let pos_prev = order
                .iter()
                .position(|x| x == &format!("s{}", i - 1))
                .unwrap();
            let pos_cur = order.iter().position(|x| x == &format!("s{i}")).unwrap();
            assert!(pos_prev < pos_cur, "s{} should come before s{i}", i - 1);
        }
    }

    #[test]
    fn empty_graph_no_services() {
        let services: HashMap<String, Vec<Dependency>> = HashMap::new();
        let graph = DependencyGraph::new(&services).unwrap();
        let order = graph.startup_order().unwrap();
        assert!(order.is_empty());
    }

    #[test]
    fn single_service_graph() {
        let mut services: HashMap<String, Vec<Dependency>> = HashMap::new();
        services.insert("only".into(), vec![]);
        let graph = DependencyGraph::new(&services).unwrap();

        let order = graph.startup_order().unwrap();
        assert_eq!(order, vec!["only"]);

        let shutdown = graph.shutdown_order().unwrap();
        assert_eq!(shutdown, vec!["only"]);
    }

    #[test]
    fn cascade_failure_on_leaf_node() {
        // A -> B -> C (leaf)
        let mut services: HashMap<String, Vec<Dependency>> = HashMap::new();
        services.insert("A".into(), vec![]);
        services.insert("B".into(), vec![simple_dep("A")]);
        services.insert("C".into(), vec![simple_dep("B")]);

        let graph = DependencyGraph::new(&services).unwrap();

        // Failing the leaf "C" should cascade to nothing (nobody depends on C)
        let affected = graph.cascade_failure("C");
        assert!(affected.is_empty(), "leaf failure should not cascade");
    }

    #[test]
    fn dependencies_satisfied_with_started_condition() {
        let mut services: HashMap<String, Vec<Dependency>> = HashMap::new();
        services.insert("a".into(), vec![]);
        services.insert("b".into(), vec![started_dep("a")]);

        let graph = DependencyGraph::new(&services).unwrap();

        // Running satisfies Started
        assert!(graph.dependencies_satisfied("b", |_| ServiceStatus::Running));

        // Healthy satisfies Started
        assert!(graph.dependencies_satisfied("b", |_| ServiceStatus::Healthy));

        // Degraded satisfies Started
        assert!(graph.dependencies_satisfied("b", |_| ServiceStatus::Degraded));

        // Stopped does not satisfy Started
        assert!(!graph.dependencies_satisfied("b", |_| ServiceStatus::Stopped));

        // Starting does not satisfy Started
        assert!(!graph.dependencies_satisfied("b", |_| ServiceStatus::Starting));
    }

    #[test]
    fn multiple_dependencies_on_same_service() {
        // "b" depends on "a" twice with different conditions
        let mut services: HashMap<String, Vec<Dependency>> = HashMap::new();
        services.insert("a".into(), vec![]);
        services.insert("b".into(), vec![simple_dep("a"), healthy_dep("a")]);

        let graph = DependencyGraph::new(&services).unwrap();

        // Running satisfies Simple/Started but not Healthy
        assert!(!graph.dependencies_satisfied("b", |_| ServiceStatus::Running));

        // Healthy satisfies both
        assert!(graph.dependencies_satisfied("b", |_| ServiceStatus::Healthy));
    }
}

// ---------------------------------------------------------------------------
// dependency module
// ---------------------------------------------------------------------------
mod dependency {
    use super::*;

    #[test]
    fn deserialize_invalid_condition_string() {
        let yaml = r#""svc bogus""#;
        let result: Result<Dependency, _> = serde_yaml::from_str(yaml);
        assert!(result.is_err(), "should reject unknown condition");
    }

    #[test]
    fn deserialize_service_started_string() {
        let yaml = r#""navigator started""#;
        let dep: Dependency = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(dep.service_name(), "navigator");
        assert_eq!(dep.condition(), DependencyCondition::Started);
    }

    #[test]
    fn round_trip_serde_simple() {
        let dep = Dependency::Simple("lidar".into());
        let yaml = serde_yaml::to_string(&dep).unwrap();
        let back: Dependency = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(dep, back);
    }

    #[test]
    fn round_trip_serde_with_started() {
        let dep = Dependency::WithCondition {
            service: "cam".into(),
            condition: DependencyCondition::Started,
        };
        let yaml = serde_yaml::to_string(&dep).unwrap();
        let back: Dependency = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(dep, back);
    }

    #[test]
    fn round_trip_serde_with_healthy() {
        let dep = Dependency::WithCondition {
            service: "db".into(),
            condition: DependencyCondition::Healthy,
        };
        let yaml = serde_yaml::to_string(&dep).unwrap();
        let back: Dependency = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(dep, back);
    }

    #[test]
    fn equality_and_hash() {
        use std::collections::HashSet;

        let a = Dependency::Simple("x".into());
        let b = Dependency::Simple("x".into());
        assert_eq!(a, b);

        let c = Dependency::WithCondition {
            service: "x".into(),
            condition: DependencyCondition::Healthy,
        };
        let d = Dependency::WithCondition {
            service: "x".into(),
            condition: DependencyCondition::Healthy,
        };
        assert_eq!(c, d);

        // Simple != WithCondition even for same service
        assert_ne!(a, c);

        // Hash set deduplication
        let mut set = HashSet::new();
        set.insert(a.clone());
        set.insert(b);
        assert_eq!(set.len(), 1, "identical deps should deduplicate");

        set.insert(c.clone());
        assert_eq!(set.len(), 2);
    }
}

// ---------------------------------------------------------------------------
// execute module
// ---------------------------------------------------------------------------
mod execute {
    use super::*;

    #[test]
    fn ros2_deserialize() {
        let yaml = r#"
type: ros2
package: nav2_bringup
launch_file: navigation.launch.py
launch_args:
  use_sim_time: "true"
  map: "/maps/office.yaml"
"#;
        let config: ExecuteConfig = serde_yaml::from_str(yaml).unwrap();
        match &config {
            ExecuteConfig::Ros2 {
                package,
                launch_file,
                launch_args,
                ..
            } => {
                assert_eq!(package, "nav2_bringup");
                assert_eq!(launch_file, "navigation.launch.py");
                assert_eq!(launch_args.get("use_sim_time").unwrap(), "true");
                assert_eq!(launch_args.get("map").unwrap(), "/maps/office.yaml");
            }
            _ => panic!("Expected Ros2 variant"),
        }
    }

    #[test]
    fn pixi_without_optional_fields() {
        let yaml = r#"
type: pixi
task: build
"#;
        let config: ExecuteConfig = serde_yaml::from_str(yaml).unwrap();
        match config {
            ExecuteConfig::Pixi {
                task,
                environment,
                stop_task,
                working_dir,
            } => {
                assert_eq!(task, "build");
                assert!(environment.is_none());
                assert!(stop_task.is_none());
                assert!(working_dir.is_none());
            }
            _ => panic!("Expected Pixi variant"),
        }
    }

    #[test]
    fn shell_with_stop_command() {
        let yaml = r#"
type: shell
command: "python server.py"
stop_command: "python server.py --stop"
"#;
        let config: ExecuteConfig = serde_yaml::from_str(yaml).unwrap();
        match config {
            ExecuteConfig::Shell {
                command,
                stop_command,
                ..
            } => {
                assert_eq!(command, "python server.py");
                assert_eq!(stop_command.unwrap(), "python server.py --stop");
            }
            _ => panic!("Expected Shell variant"),
        }
    }

    #[test]
    fn shell_with_working_dir() {
        let yaml = r#"
type: shell
command: "python run.py"
working_dir: /opt/app
"#;
        let config: ExecuteConfig = serde_yaml::from_str(yaml).unwrap();
        match config {
            ExecuteConfig::Shell { working_dir, .. } => {
                assert_eq!(working_dir, Some(PathBuf::from("/opt/app")));
            }
            _ => panic!("Expected Shell variant"),
        }
    }

    #[test]
    fn docker_with_ports() {
        let yaml = r#"
type: docker
image: nginx:latest
ports:
  - host: 8080
    container: 80
  - host: 8443
    container: 443
    protocol: udp
"#;
        let config: ExecuteConfig = serde_yaml::from_str(yaml).unwrap();
        match config {
            ExecuteConfig::Docker { ports, .. } => {
                assert_eq!(ports.len(), 2);
                assert_eq!(ports[0].host, 8080);
                assert_eq!(ports[0].container, 80);
                assert_eq!(ports[0].protocol, "tcp"); // default
                assert_eq!(ports[1].protocol, "udp");
            }
            _ => panic!("Expected Docker variant"),
        }
    }

    #[test]
    fn executor_type_for_all_variants() {
        let pixi = ExecuteConfig::Pixi {
            task: "t".into(),
            environment: None,
            stop_task: None,
            working_dir: None,
        };
        assert_eq!(pixi.executor_type(), "pixi");

        let ros2 = ExecuteConfig::Ros2 {
            package: "p".into(),
            launch_file: "l".into(),
            launch_args: HashMap::new(),
            stop_task: None,
            working_dir: None,
        };
        assert_eq!(ros2.executor_type(), "ros2");

        let shell = ExecuteConfig::Shell {
            command: "c".into(),
            stop_command: None,
            working_dir: None,
        };
        assert_eq!(shell.executor_type(), "shell");

        let docker = ExecuteConfig::Docker {
            image: "img".into(),
            volumes: vec![],
            ports: vec![],
            privileged: false,
            network: None,
        };
        assert_eq!(docker.executor_type(), "docker");
    }

    #[test]
    fn resolve_working_dir_relative_path() {
        let base = PathBuf::from("/home/user/project");

        let mut config = ExecuteConfig::Shell {
            command: "run.sh".into(),
            stop_command: None,
            working_dir: Some(PathBuf::from("src/app")),
        };
        config.resolve_working_dir(&base);

        match config {
            ExecuteConfig::Shell { working_dir, .. } => {
                assert_eq!(
                    working_dir,
                    Some(PathBuf::from("/home/user/project/src/app"))
                );
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn resolve_working_dir_absolute_path_unchanged() {
        let base = PathBuf::from("/home/user/project");

        let mut config = ExecuteConfig::Pixi {
            task: "t".into(),
            environment: None,
            stop_task: None,
            working_dir: Some(PathBuf::from("/absolute/path")),
        };
        config.resolve_working_dir(&base);

        match config {
            ExecuteConfig::Pixi { working_dir, .. } => {
                assert_eq!(working_dir, Some(PathBuf::from("/absolute/path")));
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn resolve_working_dir_docker_is_noop() {
        let base = PathBuf::from("/base");
        let mut config = ExecuteConfig::Docker {
            image: "img".into(),
            volumes: vec![],
            ports: vec![],
            privileged: false,
            network: None,
        };
        // Should not panic
        config.resolve_working_dir(&base);
    }
}

// ---------------------------------------------------------------------------
// health module
// ---------------------------------------------------------------------------
mod health {
    use super::*;

    #[test]
    fn heartbeat_timeout_detection() {
        let old_time = SystemTime::now() - Duration::from_secs(60);
        let checker = HealthChecker::Heartbeat {
            last_seen: Some(old_time),
            timeout: Duration::from_secs(5),
        };
        assert!(
            checker.is_timed_out(),
            "heartbeat 60s ago should be timed out with 5s timeout"
        );
    }

    #[test]
    fn heartbeat_not_timed_out_when_no_last_seen() {
        let checker = HealthChecker::Heartbeat {
            last_seen: None,
            timeout: Duration::from_secs(5),
        };
        assert!(
            !checker.is_timed_out(),
            "no last_seen should not be considered timed out"
        );
    }

    #[test]
    fn record_heartbeat_on_non_heartbeat_checker_returns_error() {
        let mut tcp = HealthChecker::Tcp {
            port: 8080,
            timeout: Duration::from_secs(2),
        };
        let result = tcp.record_heartbeat();
        assert!(result.is_err());

        let mut http = HealthChecker::Http {
            port: 3000,
            path: "/health".into(),
            expected_status: 200,
        };
        let result = http.record_heartbeat();
        assert!(result.is_err());

        let mut script = HealthChecker::Script {
            command: "check.sh".into(),
            timeout: Duration::from_secs(5),
        };
        let result = script.record_heartbeat();
        assert!(result.is_err());
    }

    #[test]
    fn multiple_heartbeat_recordings() {
        let mut checker = HealthChecker::Heartbeat {
            last_seen: None,
            timeout: Duration::from_secs(30),
        };

        // First heartbeat
        checker.record_heartbeat().unwrap();
        assert!(!checker.is_timed_out());

        // Simulate time passing by setting an old last_seen
        if let HealthChecker::Heartbeat {
            ref mut last_seen, ..
        } = checker
        {
            *last_seen = Some(SystemTime::now() - Duration::from_secs(60));
        }
        assert!(checker.is_timed_out());

        // Record again -- should reset and no longer be timed out
        checker.record_heartbeat().unwrap();
        assert!(!checker.is_timed_out());
    }
}

// ---------------------------------------------------------------------------
// ipc module
// ---------------------------------------------------------------------------
mod ipc {
    use super::*;

    #[test]
    fn subscribe_message_roundtrip() {
        let msg = ClientMessage::Subscribe {
            events: true,
            logs: Some("lidar".into()),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: ClientMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn subscribe_message_no_logs() {
        let msg = ClientMessage::Subscribe {
            events: false,
            logs: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: ClientMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn get_snapshot_message_roundtrip() {
        let msg = ClientMessage::GetSnapshot;
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("get_snapshot"));
        let back: ClientMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn get_logs_message_with_service() {
        let msg = ClientMessage::GetLogs {
            service: Some("navigator".into()),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: ClientMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn get_logs_message_all_services() {
        let msg = ClientMessage::GetLogs { service: None };
        let json = serde_json::to_string(&msg).unwrap();
        let back: ClientMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn status_update_server_message() {
        let msg = ServerMessage::StatusUpdate {
            service: "cam".into(),
            status: ServiceStatus::Running,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: ServerMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn log_line_server_message() {
        let msg = ServerMessage::LogLine {
            service: "lidar".into(),
            line: "sensor initialized".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: ServerMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn log_history_server_message() {
        let msg = ServerMessage::LogHistory {
            service: Some("nav".into()),
            lines: vec!["line1".into(), "line2".into()],
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: ServerMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn log_history_all_services() {
        let msg = ServerMessage::LogHistory {
            service: None,
            lines: vec![],
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: ServerMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn all_service_status_variants_serialize() {
        let variants = vec![
            ServiceStatus::Starting,
            ServiceStatus::Running,
            ServiceStatus::Healthy,
            ServiceStatus::Degraded,
            ServiceStatus::Stopping,
            ServiceStatus::Stopped,
            ServiceStatus::Failed,
        ];

        for status in variants {
            let json = serde_json::to_string(&status).unwrap();
            let back: ServiceStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, back, "roundtrip failed for {json}");
        }
    }

    #[test]
    fn all_command_action_variants_serialize() {
        let variants = vec![
            CommandAction::Start,
            CommandAction::Stop,
            CommandAction::Restart,
            CommandAction::Kill,
            CommandAction::StopDaemon,
        ];

        for action in variants {
            let json = serde_json::to_string(&action).unwrap();
            let back: CommandAction = serde_json::from_str(&json).unwrap();
            assert_eq!(action, back, "roundtrip failed for {json}");
        }
    }

    #[test]
    fn command_action_snake_case_format() {
        let json = serde_json::to_string(&CommandAction::StopDaemon).unwrap();
        assert_eq!(json, r#""stop_daemon""#);
    }

    #[test]
    fn log_level_serialization_roundtrip() {
        let levels = vec![
            LogLevel::Trace,
            LogLevel::Debug,
            LogLevel::Info,
            LogLevel::Warn,
            LogLevel::Error,
        ];

        for level in levels {
            let json = serde_json::to_string(&level).unwrap();
            let back: LogLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(level, back, "roundtrip failed for {json}");
        }
    }

    #[test]
    fn log_level_lowercase_format() {
        assert_eq!(
            serde_json::to_string(&LogLevel::Trace).unwrap(),
            r#""trace""#
        );
        assert_eq!(
            serde_json::to_string(&LogLevel::Debug).unwrap(),
            r#""debug""#
        );
        assert_eq!(serde_json::to_string(&LogLevel::Info).unwrap(), r#""info""#);
        assert_eq!(serde_json::to_string(&LogLevel::Warn).unwrap(), r#""warn""#);
        assert_eq!(
            serde_json::to_string(&LogLevel::Error).unwrap(),
            r#""error""#
        );
    }
}

// ---------------------------------------------------------------------------
// policy module
// ---------------------------------------------------------------------------
mod policy {
    use super::*;

    #[test]
    fn deserialize_kebab_case_on_failure() {
        let yaml = r#"
restart: on-failure
max_restarts: 5
restart_delay: 10s
stop_timeout: 30s
"#;
        let policy: PolicyConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(policy.restart, RestartPolicy::OnFailure);
    }

    #[test]
    fn partial_policy_with_defaults() {
        // Only restart field; the rest should use defaults
        let yaml = r#"
restart: always
"#;
        let policy: PolicyConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(policy.restart, RestartPolicy::Always);
        assert_eq!(policy.max_restarts, 0); // default
        assert_eq!(policy.restart_delay, Duration::from_secs(5)); // default
        assert_eq!(policy.stop_timeout, Duration::from_secs(10)); // default
    }

    #[test]
    fn restart_policy_all_variants_roundtrip() {
        let variants = vec![
            RestartPolicy::Always,
            RestartPolicy::OnFailure,
            RestartPolicy::Never,
        ];

        for variant in variants {
            let yaml = serde_yaml::to_string(&variant).unwrap();
            let back: RestartPolicy = serde_yaml::from_str(&yaml).unwrap();
            assert_eq!(variant, back, "roundtrip failed for {yaml}");
        }
    }

    #[test]
    fn restart_policy_kebab_case_serialization() {
        let yaml = serde_yaml::to_string(&RestartPolicy::OnFailure).unwrap();
        assert!(
            yaml.contains("on-failure"),
            "OnFailure should serialize as 'on-failure', got: {yaml}"
        );
    }

    #[test]
    fn full_policy_roundtrip() {
        let policy = PolicyConfig {
            restart: RestartPolicy::Always,
            max_restarts: 10,
            restart_delay: Duration::from_secs(30),
            stop_timeout: Duration::from_secs(60),
        };

        let yaml = serde_yaml::to_string(&policy).unwrap();
        let back: PolicyConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(policy, back);
    }
}

// ---------------------------------------------------------------------------
// process module
// ---------------------------------------------------------------------------
mod process {
    use super::*;

    #[test]
    fn pixi_command_without_environment() {
        let config = ExecuteConfig::Pixi {
            task: "build".into(),
            environment: None,
            stop_task: None,
            working_dir: None,
        };

        let cmd = build_command(&config, &HashMap::new()).unwrap();
        assert_eq!(cmd, vec!["pixi", "run", "build"]);
        // Should NOT contain "-e"
        assert!(!cmd.contains(&"-e".to_string()));
    }

    #[test]
    fn get_stop_command_pixi_with_stop_task() {
        let config = ExecuteConfig::Pixi {
            task: "start".into(),
            environment: Some("drivers".into()),
            stop_task: Some("cleanup".into()),
            working_dir: None,
        };

        let stop = get_stop_command(&config).unwrap();
        assert_eq!(stop, vec!["pixi", "run", "-e", "drivers", "cleanup"]);
    }

    #[test]
    fn get_stop_command_ros2_with_stop_task() {
        let config = ExecuteConfig::Ros2 {
            package: "nav2".into(),
            launch_file: "nav.launch.py".into(),
            launch_args: HashMap::new(),
            stop_task: Some("ros2 lifecycle set /nav2 shutdown".into()),
            working_dir: None,
        };

        let stop = get_stop_command(&config).unwrap();
        assert_eq!(stop, vec!["sh", "-c", "ros2 lifecycle set /nav2 shutdown"]);
    }

    #[test]
    fn get_stop_command_returns_none_when_missing() {
        let configs = vec![
            ExecuteConfig::Pixi {
                task: "t".into(),
                environment: None,
                stop_task: None,
                working_dir: None,
            },
            ExecuteConfig::Ros2 {
                package: "p".into(),
                launch_file: "l".into(),
                launch_args: HashMap::new(),
                stop_task: None,
                working_dir: None,
            },
            ExecuteConfig::Shell {
                command: "c".into(),
                stop_command: None,
                working_dir: None,
            },
            ExecuteConfig::Docker {
                image: "img".into(),
                volumes: vec![],
                ports: vec![],
                privileged: false,
                network: None,
            },
        ];

        for cfg in &configs {
            assert!(
                get_stop_command(cfg).is_none(),
                "expected None for {:?}",
                cfg.executor_type()
            );
        }
    }

    #[test]
    fn get_working_dir_for_each_variant() {
        let wd = Some(PathBuf::from("/work"));

        let pixi = ExecuteConfig::Pixi {
            task: "t".into(),
            environment: None,
            stop_task: None,
            working_dir: wd.clone(),
        };
        assert_eq!(get_working_dir(&pixi), wd);

        let ros2 = ExecuteConfig::Ros2 {
            package: "p".into(),
            launch_file: "l".into(),
            launch_args: HashMap::new(),
            stop_task: None,
            working_dir: wd.clone(),
        };
        assert_eq!(get_working_dir(&ros2), wd);

        let shell = ExecuteConfig::Shell {
            command: "c".into(),
            stop_command: None,
            working_dir: wd.clone(),
        };
        assert_eq!(get_working_dir(&shell), wd);

        let docker = ExecuteConfig::Docker {
            image: "img".into(),
            volumes: vec![],
            ports: vec![],
            privileged: false,
            network: None,
        };
        assert_eq!(get_working_dir(&docker), None);
    }

    #[test]
    fn docker_command_without_optional_fields() {
        let config = ExecuteConfig::Docker {
            image: "alpine:latest".into(),
            volumes: vec![],
            ports: vec![],
            privileged: false,
            network: None,
        };

        let cmd = build_command(&config, &HashMap::new()).unwrap();
        assert_eq!(cmd, vec!["docker", "run", "alpine:latest"]);
        assert!(!cmd.contains(&"--privileged".to_string()));
        assert!(!cmd.contains(&"--network".to_string()));
        assert!(!cmd.contains(&"-v".to_string()));
        assert!(!cmd.contains(&"-p".to_string()));
    }

    #[test]
    fn generate_process_name_with_hyphens_and_underscores() {
        let name = generate_process_name("my-service_v2", None).unwrap();
        assert_eq!(name, "krill.my-service_v2");

        let name = generate_process_name("a-b_c-d", Some(7)).unwrap();
        assert_eq!(name, "krill.a-b_c-d.7");
    }
}

// ---------------------------------------------------------------------------
// validation module
// ---------------------------------------------------------------------------
mod validation {
    use super::*;

    #[test]
    fn backtick_substitution_rejected() {
        let result = validate_shell_command("echo `whoami`");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("backtick") || msg.contains("Dangerous"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn or_chaining_rejected() {
        let result = validate_shell_command("true || echo fallback");
        assert!(result.is_err());
        // The FORBIDDEN_PATTERNS list checks "|" before "||", so the error
        // message will reference "pipes" rather than "OR chaining".
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("pipe") || msg.contains("OR chaining") || msg.contains("Dangerous"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn and_chaining_rejected() {
        let result = validate_shell_command("cmd1 && cmd2");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("AND chaining") || msg.contains("Dangerous"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn command_at_exactly_max_length() {
        // MAX_COMMAND_LENGTH is 4096
        let cmd = "a".repeat(4096);
        // Should be accepted (not too long)
        assert!(
            validate_shell_command(&cmd).is_ok(),
            "exactly 4096 chars should be accepted"
        );

        // One more char should be rejected
        let too_long = "a".repeat(4097);
        assert!(validate_shell_command(&too_long).is_err());
    }

    #[test]
    fn flag_that_looks_like_redirect_but_is_not() {
        // "--output" does NOT contain the literal ">" character, so the
        // substring-based validator correctly allows it.
        let result = validate_shell_command("python train.py --output results.csv");
        assert!(
            result.is_ok(),
            "flag '--output' should be allowed: it contains no literal '>'"
        );

        // In contrast, an actual redirect in the command is still caught.
        let result = validate_shell_command("python train.py --output > results.csv");
        assert!(result.is_err());
    }
}

// ---------------------------------------------------------------------------
// Cross-module: config -> dag integration
// ---------------------------------------------------------------------------
mod cross_module {
    use super::*;

    /// Load a config from YAML, then feed its dependency information into
    /// a DependencyGraph and verify the startup order.
    #[test]
    fn config_feeds_dag_correctly() {
        let yaml = r#"
version: "1"
name: workspace
services:
  base:
    execute:
      type: pixi
      task: run-base
  mid:
    execute:
      type: pixi
      task: run-mid
    dependencies:
      - base
  top:
    execute:
      type: pixi
      task: run-top
    dependencies:
      - "mid healthy"
"#;
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(yaml.as_bytes()).unwrap();

        let cfg = KrillConfig::from_file(&f.path().to_path_buf()).unwrap();

        // Build dependency map from config
        let dep_map: HashMap<String, Vec<Dependency>> = cfg
            .services
            .iter()
            .map(|(name, svc)| (name.clone(), svc.dependencies.clone()))
            .collect();

        let graph = DependencyGraph::new(&dep_map).unwrap();
        let order = graph.startup_order().unwrap();

        let pos = |s: &str| order.iter().position(|x| x == s).unwrap();
        assert!(pos("base") < pos("mid"));
        assert!(pos("mid") < pos("top"));
    }

    /// Verify that the DAG correctly uses the healthy condition from config
    /// when checking dependency satisfaction.
    #[test]
    fn config_healthy_dep_checked_by_dag() {
        let yaml = r#"
version: "1"
name: ws
services:
  db:
    execute:
      type: pixi
      task: start-db
  app:
    execute:
      type: pixi
      task: start-app
    dependencies:
      - "db healthy"
"#;
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(yaml.as_bytes()).unwrap();

        let cfg = KrillConfig::from_file(&f.path().to_path_buf()).unwrap();

        let dep_map: HashMap<String, Vec<Dependency>> = cfg
            .services
            .iter()
            .map(|(name, svc)| (name.clone(), svc.dependencies.clone()))
            .collect();

        let graph = DependencyGraph::new(&dep_map).unwrap();

        // Running alone is not enough for a healthy dep
        assert!(!graph.dependencies_satisfied("app", |_| ServiceStatus::Running));

        // Healthy satisfies the dep
        assert!(graph.dependencies_satisfied("app", |_| ServiceStatus::Healthy));
    }

    /// Verify that the process module can build commands from configs loaded
    /// from YAML.
    #[test]
    fn config_to_process_command() {
        let yaml = r#"
version: "1"
name: ws
services:
  svc:
    execute:
      type: pixi
      task: run-service
      environment: prod
"#;
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(yaml.as_bytes()).unwrap();

        let cfg = KrillConfig::from_file(&f.path().to_path_buf()).unwrap();
        let svc = &cfg.services["svc"];

        let cmd = build_command(&svc.execute, &cfg.env).unwrap();
        assert_eq!(cmd, vec!["pixi", "run", "-e", "prod", "run-service"]);

        let name = generate_process_name("svc", None).unwrap();
        assert_eq!(name, "krill.svc");
    }
}

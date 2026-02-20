// Integration tests for krill-daemon crate

use std::collections::HashMap;
use std::time::Duration;

use krill_common::{
    ExecuteConfig, KrillConfig, PolicyConfig, RestartPolicy, ServiceConfig, ServiceStatus,
};
use krill_daemon::runner::ServiceState;
use krill_daemon::{LogStore, Orchestrator, ServiceRunner};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a minimal ServiceConfig with the given RestartPolicy and max_restarts.
fn make_service_config(policy: RestartPolicy, max_restarts: u32) -> ServiceConfig {
    ServiceConfig {
        execute: ExecuteConfig::Shell {
            command: "echo hello".to_string(),
            stop_command: None,
            working_dir: None,
        },
        dependencies: vec![],
        critical: false,
        gpu: false,
        health_check: None,
        policy: PolicyConfig {
            restart: policy,
            max_restarts,
            restart_delay: Duration::from_secs(1),
            stop_timeout: Duration::from_secs(5),
        },
    }
}

/// Build a minimal ServiceConfig with the default OnFailure policy.
fn make_default_service_config() -> ServiceConfig {
    make_service_config(RestartPolicy::OnFailure, 3)
}

/// Build a ServiceRunner with sensible defaults for testing.
fn make_runner(name: &str, config: ServiceConfig) -> ServiceRunner {
    ServiceRunner::new(
        name.to_string(),
        "test-workspace".to_string(),
        config,
        HashMap::new(),
    )
}

/// Build a minimal KrillConfig with a single service.
fn make_single_service_krill_config() -> KrillConfig {
    let mut services = HashMap::new();
    services.insert("svc-a".to_string(), make_default_service_config());

    KrillConfig {
        version: "1".to_string(),
        name: "test-workspace".to_string(),
        log_dir: None,
        env: HashMap::new(),
        services,
    }
}

// ===========================================================================
// LogStore tests
// ===========================================================================

mod log_store_tests {
    use super::*;

    #[test]
    fn test_create_log_store_with_custom_directory() {
        let tmp = TempDir::new().unwrap();
        let store = LogStore::new(Some(tmp.path().to_path_buf())).unwrap();

        // The session directory should exist inside our custom base dir.
        assert!(store.session_dir().exists());
        assert!(store.session_dir().starts_with(tmp.path()));
    }

    #[tokio::test]
    async fn test_add_logs_for_multiple_services_and_retrieve() {
        let tmp = TempDir::new().unwrap();
        let store = LogStore::new(Some(tmp.path().to_path_buf())).unwrap();

        store.add_log("svc-a", "line-a-1".to_string()).await;
        store.add_log("svc-a", "line-a-2".to_string()).await;
        store.add_log("svc-b", "line-b-1".to_string()).await;

        let logs_a = store.get_logs(Some("svc-a"), 100).await;
        assert_eq!(logs_a.len(), 2);
        assert_eq!(logs_a[0], "line-a-1");
        assert_eq!(logs_a[1], "line-a-2");

        let logs_b = store.get_logs(Some("svc-b"), 100).await;
        assert_eq!(logs_b.len(), 1);
        assert_eq!(logs_b[0], "line-b-1");
    }

    #[tokio::test]
    async fn test_log_retrieval_with_limit() {
        let tmp = TempDir::new().unwrap();
        let store = LogStore::new(Some(tmp.path().to_path_buf())).unwrap();

        for i in 0..10 {
            store.add_log("svc-a", format!("line-{}", i)).await;
        }

        // Ask for only 3 lines -- should get the *last* 3 (most recent).
        let logs = store.get_logs(Some("svc-a"), 3).await;
        assert_eq!(logs.len(), 3);
        assert_eq!(logs[0], "line-7");
        assert_eq!(logs[1], "line-8");
        assert_eq!(logs[2], "line-9");
    }

    #[tokio::test]
    async fn test_get_logs_for_all_services_returns_interleaved_output() {
        let tmp = TempDir::new().unwrap();
        let store = LogStore::new(Some(tmp.path().to_path_buf())).unwrap();

        store.add_log("svc-a", "hello from a".to_string()).await;
        store.add_log("svc-b", "hello from b".to_string()).await;

        // Passing None returns logs from all services, prefixed with [service].
        let all = store.get_logs(None, 100).await;
        assert!(all.len() >= 2);

        // Every line should be prefixed with a service tag.
        for line in &all {
            assert!(
                line.starts_with("[svc-a]") || line.starts_with("[svc-b]"),
                "Expected service prefix, got: {}",
                line
            );
        }
    }

    #[test]
    fn test_session_directory_contains_expected_files() {
        let tmp = TempDir::new().unwrap();
        let store = LogStore::new(Some(tmp.path().to_path_buf())).unwrap();

        let session = store.session_dir();
        assert!(session.join("timeline.jsonl").exists());
        assert!(session.join("krill.log").exists());
    }

    #[tokio::test]
    async fn test_log_daemon_events() {
        let tmp = TempDir::new().unwrap();
        let store = LogStore::new(Some(tmp.path().to_path_buf())).unwrap();

        // log_daemon writes to krill.log and timeline.jsonl
        store
            .log_daemon(krill_daemon::logging::LogLevel::Info, "daemon started")
            .await;

        let daemon_log_path = store.session_dir().join("krill.log");
        let contents = std::fs::read_to_string(&daemon_log_path).unwrap();
        assert!(
            contents.contains("daemon started"),
            "krill.log should contain the daemon message"
        );

        // timeline.jsonl should also have an entry
        let timeline_path = store.session_dir().join("timeline.jsonl");
        let timeline = std::fs::read_to_string(&timeline_path).unwrap();
        assert!(
            timeline.contains("daemon started"),
            "timeline.jsonl should contain the daemon message"
        );
    }
}

// ===========================================================================
// ServiceRunner tests
// ===========================================================================

mod service_runner_tests {
    use super::*;

    #[test]
    fn test_new_runner_initial_state_is_pending() {
        let runner = make_runner("test-svc", make_default_service_config());
        assert_eq!(runner.state(), ServiceState::Pending);
    }

    #[test]
    fn test_update_health_running_to_healthy() {
        let config = make_service_config(RestartPolicy::OnFailure, 3);
        let mut runner = make_runner("svc", config);

        // Manually set state to Running via start path is complex, so we
        // use the public update_health which only transitions from certain
        // states. We need the runner in Running state first.
        //
        // ServiceRunner::new sets state to Pending. update_health only acts
        // on Running, Healthy, and Degraded. We can work around this by
        // noting that the struct field `state` starts as Pending and
        // update_health(true) from Pending is a no-op. We need a way to get
        // to Running.
        //
        // The service_name and config are public fields, and the runner
        // module is public. However, `state` is private. The only way to
        // reach Running without spawning a real process is not available
        // through the public API -- so we verify the transitions that are
        // reachable.
        //
        // Approach: we cannot directly set state to Running without
        // start(). Instead, test the Healthy->Degraded and Degraded->Healthy
        // transitions which are reachable after a start(). For the
        // Running->Healthy transition, we use tokio::test and actually call
        // start() with a long-running command.

        // From Pending, update_health should be a no-op.
        runner.update_health(true);
        assert_eq!(runner.state(), ServiceState::Pending);
    }

    #[tokio::test]
    async fn test_state_transitions_running_to_healthy_to_degraded_and_back() {
        // Use a command that stays alive for a bit so we can observe Running.
        let config = ServiceConfig {
            execute: ExecuteConfig::Shell {
                command: "sleep 60".to_string(),
                stop_command: None,
                working_dir: None,
            },
            dependencies: vec![],
            critical: false,
            gpu: false,
            health_check: None,
            policy: PolicyConfig {
                restart: RestartPolicy::OnFailure,
                max_restarts: 3,
                restart_delay: Duration::from_secs(1),
                stop_timeout: Duration::from_secs(2),
            },
        };

        let mut runner = make_runner("sleep-svc", config);
        assert_eq!(runner.state(), ServiceState::Pending);

        // Start the process to reach Running
        runner.start().await.unwrap();
        assert_eq!(runner.state(), ServiceState::Running);

        // Running + healthy -> Healthy
        runner.update_health(true);
        assert_eq!(runner.state(), ServiceState::Healthy);

        // Healthy + unhealthy -> Degraded
        runner.update_health(false);
        assert_eq!(runner.state(), ServiceState::Degraded);

        // Degraded + healthy -> Healthy
        runner.update_health(true);
        assert_eq!(runner.state(), ServiceState::Healthy);

        // Clean up
        runner.stop().await.unwrap();
        assert_eq!(runner.state(), ServiceState::Stopped);
    }

    #[test]
    fn test_should_restart_never_returns_false() {
        let config = make_service_config(RestartPolicy::Never, 0);
        let runner = make_runner("svc", config);

        assert!(!runner.should_restart(Some(0)));
        assert!(!runner.should_restart(Some(1)));
        assert!(!runner.should_restart(None));
    }

    #[test]
    fn test_should_restart_always_returns_true() {
        let config = make_service_config(RestartPolicy::Always, 0);
        let runner = make_runner("svc", config);

        // max_restarts == 0 means unlimited
        assert!(runner.should_restart(Some(0)));
        assert!(runner.should_restart(Some(1)));
        assert!(runner.should_restart(None));
    }

    #[test]
    fn test_should_restart_always_respects_max_restarts() {
        let config = make_service_config(RestartPolicy::Always, 2);
        let mut runner = make_runner("svc", config);

        // restart_count starts at 0, max_restarts is 2 -- should restart
        assert!(runner.should_restart(Some(1)));

        // Simulate failures to increment the counter
        runner.mark_failed(None); // restart_count -> 1
        assert!(runner.should_restart(Some(1)));

        runner.mark_failed(None); // restart_count -> 2
                                  // Now restart_count (2) >= max_restarts (2), should NOT restart
        assert!(!runner.should_restart(Some(1)));
    }

    #[test]
    fn test_should_restart_on_failure_exit_code_0_returns_false() {
        let config = make_service_config(RestartPolicy::OnFailure, 3);
        let runner = make_runner("svc", config);

        // Exit code 0 is success -- OnFailure should not restart.
        assert!(!runner.should_restart(Some(0)));
    }

    #[test]
    fn test_should_restart_on_failure_exit_code_1_returns_true() {
        let config = make_service_config(RestartPolicy::OnFailure, 3);
        let runner = make_runner("svc", config);

        // Exit code 1 is failure -- OnFailure should restart.
        assert!(runner.should_restart(Some(1)));
    }

    #[test]
    fn test_mark_failed_increments_restart_count() {
        let config = make_default_service_config();
        let mut runner = make_runner("svc", config);

        assert_eq!(runner.restart_count(), 0);

        runner.mark_failed(Some("test error".into()));
        assert_eq!(runner.restart_count(), 1);
        assert_eq!(runner.state(), ServiceState::Failed);
        assert_eq!(runner.last_error(), Some("test error"));

        runner.mark_failed(None);
        assert_eq!(runner.restart_count(), 2);
        assert_eq!(runner.last_error(), None);
    }

    #[test]
    fn test_get_status_maps_states_correctly() {
        // We can only directly observe Pending -> status mapping and
        // Failed -> status mapping through the public API.
        let config = make_default_service_config();
        let mut runner = make_runner("svc", config);

        // Pending -> Starting
        assert_eq!(runner.get_status(), ServiceStatus::Starting);

        // After mark_failed -> Failed
        runner.mark_failed(None);
        assert_eq!(runner.get_status(), ServiceStatus::Failed);
    }

    #[tokio::test]
    async fn test_get_status_running_and_stopped() {
        let config = ServiceConfig {
            execute: ExecuteConfig::Shell {
                command: "sleep 60".to_string(),
                stop_command: None,
                working_dir: None,
            },
            dependencies: vec![],
            critical: false,
            gpu: false,
            health_check: None,
            policy: PolicyConfig {
                restart: RestartPolicy::Never,
                max_restarts: 0,
                restart_delay: Duration::from_secs(1),
                stop_timeout: Duration::from_secs(2),
            },
        };
        let mut runner = make_runner("svc", config);

        runner.start().await.unwrap();
        assert_eq!(runner.get_status(), ServiceStatus::Running);

        runner.stop().await.unwrap();
        assert_eq!(runner.get_status(), ServiceStatus::Stopped);
    }

    #[test]
    fn test_executor_type_returns_correct_string() {
        // Shell executor
        let shell_config = make_default_service_config();
        let runner = make_runner("svc", shell_config);
        assert_eq!(runner.executor_type(), "shell");

        // Pixi executor
        let pixi_config = ServiceConfig {
            execute: ExecuteConfig::Pixi {
                task: "start".to_string(),
                environment: None,
                stop_task: None,
                working_dir: None,
            },
            dependencies: vec![],
            critical: false,
            gpu: false,
            health_check: None,
            policy: PolicyConfig::default(),
        };
        let runner = make_runner("pixi-svc", pixi_config);
        assert_eq!(runner.executor_type(), "pixi");
    }

    #[test]
    fn test_namespace_returns_workspace_name() {
        let config = make_default_service_config();
        let runner = make_runner("svc", config);
        assert_eq!(runner.namespace(), "test-workspace");
    }

    #[test]
    fn test_pid_is_none_before_start() {
        let config = make_default_service_config();
        let runner = make_runner("svc", config);
        assert!(runner.pid().is_none());
    }

    #[test]
    fn test_increment_restart_count() {
        let config = make_default_service_config();
        let mut runner = make_runner("svc", config);

        assert_eq!(runner.restart_count(), 0);
        runner.increment_restart_count();
        assert_eq!(runner.restart_count(), 1);
        runner.increment_restart_count();
        assert_eq!(runner.restart_count(), 2);
    }
}

// ===========================================================================
// Orchestrator tests
// ===========================================================================

mod orchestrator_tests {
    use super::*;
    use krill_common::Dependency;
    use tokio::sync::mpsc;

    #[test]
    fn test_create_orchestrator_with_valid_single_service_config() {
        let config = make_single_service_krill_config();
        let (event_tx, _event_rx) = mpsc::unbounded_channel();

        let orchestrator = Orchestrator::new(config, event_tx);
        assert!(
            orchestrator.is_ok(),
            "Orchestrator should be created successfully with a valid config"
        );
    }

    #[test]
    fn test_create_orchestrator_with_circular_deps_returns_error() {
        let mut services = HashMap::new();

        // svc-a depends on svc-b
        let mut config_a = make_default_service_config();
        config_a.dependencies = vec![Dependency::Simple("svc-b".to_string())];
        services.insert("svc-a".to_string(), config_a);

        // svc-b depends on svc-a -- circular!
        let mut config_b = make_default_service_config();
        config_b.dependencies = vec![Dependency::Simple("svc-a".to_string())];
        services.insert("svc-b".to_string(), config_b);

        let config = KrillConfig {
            version: "1".to_string(),
            name: "circular-workspace".to_string(),
            log_dir: None,
            env: HashMap::new(),
            services,
        };

        let (event_tx, _event_rx) = mpsc::unbounded_channel();
        let result = Orchestrator::new(config, event_tx);

        assert!(
            result.is_err(),
            "Orchestrator::new should fail with circular dependencies"
        );

        let err_msg = match result {
            Err(e) => e.to_string(),
            Ok(_) => panic!("Expected error but got Ok"),
        };
        assert!(
            err_msg.contains("Circular dependency"),
            "Error should mention circular dependency, got: {}",
            err_msg
        );
    }

    #[tokio::test]
    async fn test_orchestrator_get_snapshot_returns_all_services() {
        let mut services = HashMap::new();
        services.insert("svc-a".to_string(), make_default_service_config());
        services.insert("svc-b".to_string(), make_default_service_config());

        let config = KrillConfig {
            version: "1".to_string(),
            name: "snap-workspace".to_string(),
            log_dir: None,
            env: HashMap::new(),
            services,
        };

        let (event_tx, _event_rx) = mpsc::unbounded_channel();
        let orchestrator = Orchestrator::new(config, event_tx).unwrap();

        let snapshot = orchestrator.get_snapshot().await;
        assert_eq!(snapshot.len(), 2);
        assert!(snapshot.contains_key("svc-a"));
        assert!(snapshot.contains_key("svc-b"));

        // Both should be in Starting (mapped from Pending) state.
        assert_eq!(snapshot["svc-a"].status, ServiceStatus::Starting);
        assert_eq!(snapshot["svc-b"].status, ServiceStatus::Starting);
    }
}

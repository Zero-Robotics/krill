# Krill Implementation Roadmap v2

**Target:** Professional-grade robotics process orchestrator  
**Language:** Rust  
**Architecture:** Daemon-client with multi-crate workspace

---

## Project Overview

Krill is a safety-critical process orchestrator for robotics. It manages complex dependency graphs of programs (pixi tasks, ROS2 launch files, shell commands) on robotic systems.

**Core architecture:**
- `krill-daemon` — Background supervisor that manages all processes
- `krill-tui` — Terminal UI that connects to daemon (closing TUI does NOT kill daemon)
- `krill-sdk-rust` — Rust library for applications to send heartbeats
- `krill-common` — Shared types, schemas, IPC protocol
- `krill-cpp` — Header-only C++ SDK

**Key behaviors:**
- TUI is a view only—closing it does NOT stop the daemon
- Explicit "Stop Daemon" button (with confirmation) required to stop daemon
- Process naming: `{workspace}.{service}.{uuid}` (e.g., `pulsar.lidar.a1b2c3`)
- Safe by default—shell commands validated, no pipes/subshells allowed
- Docker type present in schema but returns "requires Pro" error

---

## Phase 0: Workspace Setup

**Goal:** Establish Rust workspace and CI before writing business logic.

### 0.1 Workspace Structure

```
krill/
├── Cargo.toml              # Workspace manifest
├── justfile                # Task runner
├── .github/workflows/ci.yml
├── crates/
│   ├── krill-common/       # Shared types
│   ├── krill-daemon/       # Supervisor
│   ├── krill-tui/          # Terminal UI
│   └── krill-sdk-rust/     # Heartbeat client
├── sdk/
│   └── krill-cpp/          # C++ header-only SDK
├── schemas/
│   └── krill.schema.json   # JSON schema for editors
├── examples/
│   └── pulsar.yaml         # Example config
└── tests/
    └── integration/
```

### 0.2 Workspace Cargo.toml

```toml
[workspace]
resolver = "2"
members = [
    "crates/krill-common",
    "crates/krill-daemon",
    "crates/krill-tui",
    "crates/krill-sdk-rust",
]

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "Apache-2.0"

[workspace.dependencies]
tokio = { version = "1.40", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
serde_yaml = "0.9"
thiserror = "1.0"
anyhow = "1.0"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
clap = { version = "4.5", features = ["derive"] }
ratatui = "0.28"
crossterm = "0.28"
nix = { version = "0.29", features = ["signal", "process"] }
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1.10", features = ["v4"] }
humantime-serde = "1.1"
dirs = "5.0"
futures = "0.3"
reqwest = { version = "0.12", features = ["json"] }
```

### Acceptance Criteria — Phase 0

- [ ] `cargo build --workspace` succeeds with zero warnings
- [ ] `cargo test --workspace` passes
- [ ] CI pipeline runs on GitHub

---

## Phase 1: krill-common — Shared Types

**Goal:** Define all configuration types, state machine, and IPC protocol.

### 1.1 Execute Types (Tagged Enum)

The key change in v2: `execute` is a tagged enum with `type` field.

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ExecuteConfig {
    Pixi {
        task: String,
        #[serde(default)]
        environment: Option<String>,  // Defaults to service name
        stop_task: Option<String>,
        working_dir: Option<PathBuf>,
    },
    Ros2 {
        package: String,
        launch_file: String,
        #[serde(default)]
        launch_args: HashMap<String, String>,
        stop_task: Option<String>,  // Can use pixi task for graceful stop
        working_dir: Option<PathBuf>,
    },
    Shell {
        command: String,            // Validated - no pipes, subshells
        stop_command: Option<String>,
        working_dir: Option<PathBuf>,
    },
    Docker {                        // Schema-valid but returns Pro error
        image: String,
        #[serde(default)]
        volumes: Vec<String>,
        #[serde(default)]
        ports: Vec<String>,
        #[serde(default)]
        privileged: bool,
        network: Option<String>,
    },
}
```

### 1.2 Dependencies (Simple + Condition Syntax)

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Dependency {
    Simple(String),                              // "lidar" → started
    WithCondition(HashMap<String, DependencyCondition>),  // "lidar: healthy"
}

impl Dependency {
    pub fn service_name(&self) -> &str {
        match self {
            Dependency::Simple(name) => name,
            Dependency::WithCondition(map) => map.keys().next().unwrap(),
        }
    }
    
    pub fn condition(&self) -> DependencyCondition {
        match self {
            Dependency::Simple(_) => DependencyCondition::Started,
            Dependency::WithCondition(map) => *map.values().next().unwrap(),
        }
    }
}
```

### 1.3 Policy Block

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PolicyConfig {
    #[serde(default)]
    pub restart: RestartPolicy,     // never | always | on-failure
    #[serde(default)]
    pub max_restarts: u32,          // 0 = unlimited
    #[serde(default = "default_restart_delay", with = "humantime_serde")]
    pub restart_delay: Duration,    // Default: 1s
    #[serde(default = "default_stop_timeout", with = "humantime_serde")]
    pub stop_timeout: Duration,     // Default: 10s
}
```

### 1.4 Shell Command Validator

```rust
pub fn validate_shell_command(cmd: &str) -> Result<(), ConfigError> {
    let forbidden = [
        ("|", "pipes"),
        (";", "semicolon chaining"),
        ("&&", "AND chaining"),
        ("||", "OR chaining"),
        ("$(", "command substitution"),
        ("`", "backtick substitution"),
        (">", "output redirection"),
        ("<", "input redirection"),
        ("&", "background execution"),
    ];
    
    for (pattern, desc) in forbidden {
        if cmd.contains(pattern) {
            return Err(ConfigError::UnsafeShellCommand {
                reason: format!("Contains {} ('{}'). Use a pixi task instead.", desc, pattern),
            });
        }
    }
    Ok(())
}
```

### 1.5 IPC Protocol

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Heartbeat {
        service: String,
        status: HeartbeatStatus,
        metadata: HashMap<String, serde_json::Value>,
    },
    Command {
        action: CommandAction,
        target: Option<String>,
    },
    Subscribe { events: bool, logs: Option<String> },
    GetSnapshot,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandAction {
    Start,
    Stop,
    Restart,
    Kill,
    StopDaemon,  // Explicit daemon stop (TUI button)
}
```

### Acceptance Criteria — Phase 1

- [ ] YAML with `type: pixi` parses correctly
- [ ] YAML with `type: docker` parses but fails validation with "requires Pro"
- [ ] Shell command `python script.py --arg` passes validation
- [ ] Shell command `cmd1 | cmd2` fails validation
- [ ] Dependency `- lidar` parses as started condition
- [ ] Dependency `- lidar: healthy` parses as healthy condition

---

## Phase 2: DAG Resolution

**Goal:** Build dependency graph, detect cycles, compute startup/shutdown order.

```rust
pub struct DependencyGraph {
    edges: HashMap<String, Vec<(String, DependencyCondition)>>,
    reverse_edges: HashMap<String, Vec<String>>,
}

impl DependencyGraph {
    pub fn startup_order(&self) -> Vec<Vec<String>>;   // Layers for concurrent start
    pub fn shutdown_order(&self) -> Vec<Vec<String>>;  // Reverse of startup
    pub fn cascade_failure(&self, failed: &str) -> Vec<String>;  // Transitive dependents
}
```

### Acceptance Criteria — Phase 2

- [ ] A→B→C produces startup order: [[C], [B], [A]]
- [ ] Cycle A→B→A returns `CyclicDependency` error
- [ ] Shutdown order is reverse of startup order
- [ ] `cascade_failure("lidar")` returns all services that depend on lidar

---

## Phase 3: Process Management

**Goal:** Spawn processes with PGID isolation, generate process names.

### 3.1 Process Naming

```rust
pub fn generate_process_name(workspace: &str, service: &str) -> String {
    let uuid = &Uuid::new_v4().to_string()[..6];
    format!("{}.{}.{}", workspace, service, uuid)
}
// Example: "pulsar.lidar.a1b2c3"
```

### 3.2 Command Builder

```rust
pub fn build_command(execute: &ExecuteConfig, service_name: &str) -> CommandSpec {
    match execute {
        ExecuteConfig::Pixi { task, environment, .. } => {
            let env = environment.as_deref().unwrap_or(service_name);
            CommandSpec {
                program: "pixi".to_string(),
                args: vec!["run", "-e", env, task],
            }
        }
        ExecuteConfig::Ros2 { package, launch_file, launch_args, .. } => {
            let mut args = vec!["launch", package, launch_file];
            for (k, v) in launch_args {
                args.push(&format!("{}:={}", k, v));
            }
            CommandSpec { program: "ros2".to_string(), args }
        }
        ExecuteConfig::Shell { command, .. } => {
            let parts: Vec<&str> = command.split_whitespace().collect();
            CommandSpec {
                program: parts[0].to_string(),
                args: parts[1..].to_vec(),
            }
        }
        ExecuteConfig::Docker { .. } => {
            panic!("Docker requires Pro"); // Caught earlier in validation
        }
    }
}
```

### 3.3 PGID Isolation

```rust
unsafe {
    command.pre_exec(|| {
        setpgid(Pid::from_raw(0), Pid::from_raw(0))?;
        Ok(())
    });
}

// Termination sends signal to process GROUP
signal::killpg(pgid, Signal::SIGTERM);
```

### Acceptance Criteria — Phase 3

- [ ] Spawned process has its own PGID (verify: `ps -o pgid`)
- [ ] Process name shows as `workspace.service.uuid` 
- [ ] Killing service kills all children (no orphans)
- [ ] SIGTERM → timeout → SIGKILL escalation works

---

## Phase 4: Service Runner & Health Checks

**Goal:** Manage individual service lifecycle with health checking.

### 4.1 Health Check Types

```rust
pub enum HealthChecker {
    Heartbeat { last_seen: Option<Instant>, timeout: Duration },
    Tcp { port: u16, timeout: Duration },
    Http { port: u16, path: String, expected_status: u16 },
    Script { command: String, timeout: Duration },  // Command validated
}
```

### 4.2 GPU Validation

```rust
pub fn validate_gpu_available() -> Result<(), String> {
    if Path::new("/dev/nvidia0").exists() { return Ok(()); }
    if env::var("CUDA_VISIBLE_DEVICES").is_ok() { return Ok(()); }
    if Command::new("nvidia-smi").output()?.status.success() { return Ok(()); }
    Err("No GPU detected".to_string())
}
```

### Acceptance Criteria — Phase 4

- [ ] Service transitions: Pending → Starting → Running → Healthy
- [ ] Heartbeat timeout → Faulted
- [ ] TCP check connects to port
- [ ] HTTP check validates status code
- [ ] GPU validation runs before starting gpu: true services
- [ ] max_restarts enforced (stops restarting after N failures)
- [ ] Restart count resets after 60s healthy

---

## Phase 5: Daemon Orchestrator

**Goal:** Coordinate all services, handle DAG startup/shutdown, fault cascading.

### 5.1 Startup Sequence

```rust
async fn start_all_services(&self) {
    for layer in self.dag.startup_order() {
        // Wait for dependencies to meet condition
        // Start all services in layer concurrently
        join_all(layer.iter().map(|s| self.start_when_ready(s))).await;
    }
}

async fn start_when_ready(&self, service: &str) {
    // Wait for each dependency to reach required condition
    for dep in &self.config.services[service].dependencies {
        match dep.condition() {
            Started => wait_for_state(Running | Healthy | Degraded),
            Healthy => wait_for_state(Healthy),
        }
    }
    // Validate GPU if required
    if self.config.services[service].gpu {
        validate_gpu_available()?;
    }
    // Start the service
    self.runners[service].start().await;
}
```

### 5.2 Critical Service Handling

```rust
async fn handle_fault(&self, service: &str) {
    if self.config.services[service].critical {
        self.emergency_stop().await;  // Stop EVERYTHING
        return;
    }
    // Cascade: stop all dependents
    for dependent in self.dag.cascade_failure(service) {
        self.runners[dependent].stop().await;
    }
}
```

### 5.3 Graceful Shutdown

```rust
async fn shutdown(&self) {
    for layer in self.dag.shutdown_order() {
        join_all(layer.iter().map(|s| self.runners[s].stop())).await;
    }
}
```

### Acceptance Criteria — Phase 5

- [ ] Services start in correct DAG order
- [ ] Services in same layer start concurrently
- [ ] `condition: healthy` waits for health check
- [ ] Critical service failure → emergency stop all
- [ ] Non-critical failure → cascade stop dependents only
- [ ] Ctrl+C → graceful shutdown in reverse order

---

## Phase 6: IPC Server & Logging

**Goal:** Unix socket server and unified logging.

### 6.1 IPC Server

- Socket at `/tmp/krill.sock` with 0600 permissions
- Newline-delimited JSON messages
- Broadcasts state changes to all connected clients
- Handles commands from TUI (start, stop, restart, kill, stop_daemon)

### 6.2 Logging

```
~/.krill/logs/session-{timestamp}/
├── krill.log           # Daemon events
├── lidar.log           # Per-service logs
├── navigator.log
└── timeline.jsonl      # Merged timeline for debugging
```

### Acceptance Criteria — Phase 6

- [ ] Socket created with correct permissions
- [ ] Multiple TUI clients can connect
- [ ] State changes broadcast to all clients
- [ ] Logs written to session directory
- [ ] timeline.jsonl contains merged logs with timestamps

---

## Phase 7: TUI Implementation

**Goal:** k9s-style terminal interface that connects to daemon.

### 7.1 Critical Behavior

**TUI does NOT control daemon lifecycle:**
- Closing TUI (q) leaves daemon running
- Explicit "Stop Daemon" button (S) with confirmation dialog required
- TUI reconnects if daemon restarts

### 7.2 Views

**List View:**
```
┌─ krill ─────────────────────────────────────────────────────┐
│ pulsar │ 4 services │ uptime 2h 34m                         │
├─────────────────────────────────────────────────────────────┤
│  SERVICE      STATE      PID     RESTARTS                   │
│▶ lidar        ●healthy   12345   0                          │
│  navigator    ●healthy   12346   0                          │
│  controller   ●running   12347   1                          │
│  vision       ○degraded  12348   3                          │
├─────────────────────────────────────────────────────────────┤
│ [↑↓]select [enter]logs [r]estart [s]top [S]stop-daemon [q]  │
└─────────────────────────────────────────────────────────────┘
```

**Log View:** Full-screen log tail for selected service
**Detail View:** Service config, state history, metadata

### 7.3 Key Bindings

| Key | Action |
|-----|--------|
| ↑/k | Previous service |
| ↓/j | Next service |
| Enter | Log view |
| d | Detail view |
| r | Restart service |
| s | Stop service |
| S | Stop daemon (confirmation required) |
| q | Quit TUI (daemon keeps running) |

### Acceptance Criteria — Phase 7

- [ ] TUI connects and shows service list
- [ ] State colors: green=healthy, yellow=running/degraded, red=faulted
- [ ] Arrow keys navigate
- [ ] r restarts selected service
- [ ] s stops selected service  
- [ ] q quits TUI without stopping daemon
- [ ] S shows confirmation, then stops daemon
- [ ] Real-time state updates (no polling)

---

## Phase 8: SDKs

**Goal:** Client libraries for heartbeat reporting.

### 8.1 Rust SDK

```rust
let mut client = KrillClient::new("my-service")?;
client.heartbeat()?;
client.heartbeat_with_metadata(hashmap!{ "fps" => 29.5 })?;
client.report_degraded("High latency")?;
```

### 8.2 C++ SDK (Header-Only)

```cpp
krill::Client client("my-service");
client.heartbeat();
client.heartbeat_with_metadata({{"fps", "29.5"}});
client.report_degraded("High latency");
```

### Acceptance Criteria — Phase 8

- [ ] Rust SDK compiles standalone
- [ ] C++ SDK is header-only, no dependencies
- [ ] Heartbeat received by daemon
- [ ] Degraded status transitions service state

---

## Example Config (examples/pulsar.yaml)

```yaml
version: "1"
name: pulsar
log_dir: ~/.krill/logs

env:
  ROBOT_ID: pulsar-001

services:
  lidar:
    execute:
      type: pixi
      task: start-lidar
      environment: drivers
      stop_task: stop-lidar
    critical: true
    health_check:
      type: heartbeat
      timeout: 2s
    policy:
      restart: on-failure
      max_restarts: 3

  localization:
    execute:
      type: pixi
      task: localization
      environment: slam
    dependencies:
      - lidar: healthy
    health_check:
      type: tcp
      port: 5555

  navigator:
    execute:
      type: pixi
      task: navigate
      environment: navigation
    dependencies:
      - localization: healthy
      - lidar
    health_check:
      type: http
      port: 8080
      path: /health

  vision:
    execute:
      type: pixi
      task: vision-pipeline
      environment: perception
    gpu: true
    health_check:
      type: heartbeat
      timeout: 1s
    policy:
      restart: always
```

---

## Timeline

| Phase | Duration | Deliverable |
|-------|----------|-------------|
| 0 | 2-3 days | Workspace, CI |
| 1 | 3-4 days | krill-common types |
| 2 | 2-3 days | DAG resolution |
| 3 | 4-5 days | Process management |
| 4 | 3-4 days | Health checks |
| 5 | 4-5 days | Daemon orchestrator |
| 6 | 3-4 days | IPC, logging |
| 7 | 4-5 days | TUI |
| 8 | 2 days | SDKs |
| **Total** | **~5-6 weeks** | MVP |

---

## Critical Rules

1. **PGID Isolation:** Every process MUST be in its own process group
2. **Shell Validation:** Reject pipes, subshells, redirections
3. **TUI Independence:** Closing TUI does NOT stop daemon
4. **Shutdown Order:** Always reverse DAG order
5. **Critical = Emergency Stop:** Critical failure stops EVERYTHING
6. **Process Names:** `{workspace}.{service}.{uuid}` format
7. **Docker = Pro:** Schema-valid but returns error at runtime

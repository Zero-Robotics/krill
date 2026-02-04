# Krill - Mission Control Supervisor for Robotics

Krill is a professional-grade orchestrator designed for deterministic safety and observability in robotics systems. It provides process supervision, dependency-based orchestration, and safety-critical failure handling.

## Key Features

- **Process Supervision**: Spawns child processes, monitors PIDs, and implements configurable restart policies
- **DAG Orchestration**: Ensures services start in correct order based on dependency conditions (started/healthy)
- **Safety Interceptor**: Critical service failures trigger automatic emergency stops to prevent hardware damage
- **Health Monitoring**: Dual verification via process liveness and heartbeat timeouts with configurable intervals
- **IPC Server**: Unix domain socket (`/tmp/krill.sock`) for SDK heartbeats and TUI commands
- **Process Groups**: Clean termination of entire process trees using PGIDs (negative PIDs)
- **Output Buffering**: Captures stdout/stderr with ring buffers for on-demand log viewing
- **Structured Configuration**: YAML-based service definitions with validation and dependency resolution

## Architecture Overview

```
┌─────────────────────────────────────────────────────────┐
│                     krill-tui                           │
│              (Terminal User Interface)                  │
└───────────────┬─────────────────────────────────────────┘
                │ JSON-RPC over Unix Socket
                ▼
┌─────────────────────────────────────────────────────────┐
│                     krill-daemon                        │
│            (Mission Control Supervisor)                 │
├─────────────────────────────────────────────────────────┤
│  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────┐    │
│  │Process  │  │Health   │  │Safety   │  │DAG      │    │
│  │Manager  │◄─┤Monitor  │◄─┤Interceptor│◄─┤Orchestrator│ │
│  └─────────┘  └─────────┘  └─────────┘  └─────────┘    │
│       │           │           │           │             │
│       ▼           ▼           ▼           ▼             │
│  ┌───────────────────────────────────────────────────┐  │
│  │              Service Config (YAML)               │  │
│  └───────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────┘
                │ Heartbeats (JSON)
                ▼
┌─────────────────────────────────────────────────────────┐
│                SDK Components (C++/Rust)               │
│                (Sensors, Drivers, Logic)               │
└─────────────────────────────────────────────────────────┘
```

## Quick Start

### Building from Source

```bash
# Clone the repository
git clone <repository-url>
cd krill

# Build all crates
cargo build --release

# Run tests
cargo test
```

### Example Configuration

Create a services configuration file (`services.yaml`):

```yaml
version: "1"
services:
  lidar:
    command: "/usr/bin/lidar_driver --port 9090"
    stop_cmd: "/usr/bin/lidar_driver --stop"
    restart_policy:
      condition: "on-failure"
      max_attempts: 3
      delay_sec: 2
    critical: true
    health_check:
      type: "heartbeat"
      timeout_sec: 5

  navigator:
    command: "/usr/bin/navigator --config /etc/nav.yaml"
    dependencies:
      - lidar: { condition: "healthy" }
    critical: false
    health_check:
      type: "heartbeat"
      timeout_sec: 10
```

### Running the Daemon

```bash
# Start with configuration file
./target/release/krill-daemon --config services.yaml --debug

# Or use command line options
./target/release/krill-daemon \
  --config /etc/krill/services.yaml \
  --socket /tmp/krill.sock \
  --pid-file /var/run/krill.pid \
  --log-dir ~/.krill/logs
```

## Configuration Format

### Service Definition

```yaml
service_name:
  command: "executable with args"    # Required
  stop_cmd: "graceful stop command"  # Optional
  restart_policy:                    # Optional
    condition: "always|never|on-failure"
    max_attempts: 3                  # Default: 3
    delay_sec: 2                     # Default: 2
  critical: false                    # Default: false
  health_check:                      # Optional
    type: "heartbeat|tcp|command"
    timeout_sec: 5                   # Default: 5
    port: 9090                       # Required for TCP type
    command: "health_check_cmd"      # Required for command type
  dependencies:                      # Optional list
    - dependency_name: { condition: "started|healthy" }
  environment:                       # Optional key-value pairs
    KEY: "value"
  working_directory: "/path/to/cwd"  # Optional
```

### Dependency Conditions

- **started**: Dependent service must have a running PID
- **healthy**: Dependent service must pass its health check (heartbeat received, TCP connectable, or command returns success)

### Restart Policies

- **always**: Restart service regardless of exit code
- **never**: Never restart automatically
- **on-failure**: Restart only on non-zero exit codes, respecting `max_attempts` and `delay_sec`

## IPC Communication

### Unix Socket Location
Default: `/tmp/krill.sock`

### Message Format (JSON-RPC Style)

**Heartbeat from SDK:**
```json
{
  "type": "heartbeat",
  "service": "lidar",
  "status": "healthy",
  "metadata": { "fps": 30 },
  "timestamp": "2024-02-04T10:30:00Z"
}
```

**Command from TUI:**
```json
{
  "type": "request",
  "id": "uuid-123",
  "method": "start_service",
  "params": { "name": "navigator" }
}
```

**Event to TUI:**
```json
{
  "type": "event",
  "event": "state_transition",
  "service": "lidar",
  "from": "starting",
  "to": "healthy",
  "timestamp": "2024-02-04T10:30:00Z"
}
```

### Available Commands

- `start_service` - Start a service
- `stop_service` - Stop a service gracefully
- `restart_service` - Restart a service
- `emergency_stop` - Trigger emergency shutdown
- `get_status` - Get overall system status
- `get_service_status` - Get status of specific service
- `get_service_logs` - Retrieve service output logs
- `list_services` - List all configured services
- `health_check` - Trigger manual health check
- `clear_safety_stop` - Clear safety-stopped flag

## Safety Features

### Critical Failure Handling
1. **Stop Dependents**: Immediately kill any service that depends on the failed node
2. **Escalation**: If failed service is `critical: true`, enter Global Emergency Mode
3. **Emergency Stop**: Execute global system stop command or kill all services in reverse DAG order

### Heartbeat Timing
- Standard robotics heartbeats: 10Hz (0.1s) for control, 1Hz (1.0s) for logic
- Timeout should be at least 3× the interval to avoid jitter
- "Zombie" detection via heartbeat timeout even if PID exists

### Process Group Management
- Uses negative PIDs (PGIDs) for killing entire process trees
- Prevents orphaned subprocesses during termination
- Ensures clean shutdown of shell wrappers and sub-drivers

## Development

### Project Structure

```
krill/
├── Cargo.toml                 # Workspace configuration
├── example-services.yaml      # Example configuration
├── crates/
│   ├── krill-common/         # Shared models and schemas
│   ├── krill-daemon/         # Core orchestrator daemon
│   └── krill-tui/            # Terminal UI (in development)
└── README.md
```

### Building Individual Crates

```bash
# Build common library
cargo build -p krill-common

# Build daemon
cargo build -p krill-daemon

# Build with optimizations
cargo build --release -p krill-daemon
```

### Adding a New Service

1. Define service in YAML configuration
2. Implement heartbeat in SDK component
3. Test dependency resolution
4. Verify safety interception works

## Example SDK Integration (Rust)

```rust
use krill_common::model::{HeartbeatMessage, ServiceState};
use std::time::Duration;
use tokio::time;

async fn send_heartbeats() {
    let socket_path = "/tmp/krill.sock";
    
    loop {
        let heartbeat = HeartbeatMessage::new(
            "lidar".to_string(),
            ServiceState::Healthy,
        );
        
        // Send to krill-daemon via Unix socket
        send_to_socket(socket_path, &heartbeat).await;
        
        // Send at 1Hz
        time::sleep(Duration::from_secs(1)).await;
    }
}
```

## License

[Specify license - e.g., MIT, Apache 2.0]

## Contributing

Contributions are welcome! Please see CONTRIBUTING.md for guidelines.

## Roadmap

- [ ] Complete krill-tui implementation
- [ ] Web dashboard interface
- [ ] Prometheus metrics export
- [ ] Distributed mode for multi-robot systems
- [ ] Plugin system for custom health checks
- [ ] Configuration hot-reload
- [ ] Resource limits (CPU, memory) per service

## Support

For issues and feature requests, please use the GitHub issue tracker.
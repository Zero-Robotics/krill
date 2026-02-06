# Krill Documentation

Welcome to the Krill documentation! Here you'll find everything you need to orchestrate your robotics services.

## Getting Started

- **[Quick Reference](quick-reference.md)** - Fast lookup for common configurations
- **[Configuration Guide](configuration.md)** - Complete reference for recipe files
- **[SDK Installation](sdk-installation.md)** - Install and use Krill SDKs (Python, Rust, C++)

## In-Depth Guides

- **[Health Checks Guide](health-checks.md)** - Monitor service health effectively
- **[Dependencies Guide](dependencies.md)** - Build reliable orchestration graphs
- **[SDK Installation](sdk-installation.md)** - SDK setup and usage examples

## Core Concepts

### Service Orchestration

Krill manages complex dependency graphs using a DAG (Directed Acyclic Graph):
- Services start in correct dependency order
- Health checks determine when services are ready
- Failures cascade to dependent services
- Critical services trigger emergency stops

### Execute Types

Run different types of workloads:
- **Pixi** - Python package manager tasks
- **ROS2** - Launch ROS2 nodes and packages
- **Docker** - Containerized services
- **Shell** - Validated shell commands

### Health Monitoring

Monitor service status with multiple health check types:
- **Heartbeat** - Active reporting from your code
- **TCP** - Port connection checks
- **HTTP** - REST API health endpoints
- **Script** - Custom validation logic

### Restart Policies

Control how services recover from failures:
- **never** - No automatic restarts
- **always** - Always restart
- **on-failure** - Restart only on non-zero exits

## Examples by Use Case

### Robotics

```yaml
# ROS2 navigation stack
services:
  lidar:
    execute:
      type: ros2
      package: ldlidar_ros2
      launch_file: ldlidar.launch.py
    health_check:
      type: tcp
      port: 4048

  slam:
    execute:
      type: pixi
      task: run-slam
    dependencies:
      - lidar: healthy
    health_check:
      type: heartbeat
      timeout: 5s

  navigation:
    execute:
      type: ros2
      package: nav2_bringup
      launch_file: navigation_launch.py
    dependencies:
      - slam: healthy
    critical: true
```

### Web Services

```yaml
# Full-stack web application
services:
  database:
    execute:
      type: docker
      image: postgres:15
    health_check:
      type: tcp
      port: 5432

  backend:
    execute:
      type: pixi
      task: start-api
    dependencies:
      - database: healthy
    health_check:
      type: http
      port: 8000

  frontend:
    execute:
      type: docker
      image: frontend:latest
      ports:
        - "3000:3000"
    dependencies:
      - backend: healthy
```

### Development Environment

```yaml
# Hot-reload dev setup
services:
  dev-db:
    execute:
      type: docker
      image: postgres:15
      ports:
        - "5432:5432"

  dev-backend:
    execute:
      type: shell
      command: npm run dev
      working_dir: ./backend
    dependencies:
      - dev-db: healthy
    policy:
      restart: on-failure

  dev-frontend:
    execute:
      type: shell
      command: npm run dev
      working_dir: ./frontend
    dependencies:
      - dev-backend: started
```

## SDK Integration

### Rust

```rust
use krill_sdk_rust::KrillClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = KrillClient::new("my-service").await?;
    
    loop {
        // Your logic here
        client.heartbeat().await?;
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
}
```

### Python

```python
from krill import KrillClient

with KrillClient("my-service") as client:
    while True:
        # Your logic here
        client.heartbeat()
        time.sleep(1)
```

### C++

```cpp
#include "krill.hpp"

int main() {
    krill::Client client("my-service");
    
    while (true) {
        // Your logic here
        client.heartbeat();
        std::this_thread::sleep_for(std::chrono::seconds(1));
    }
}
```

## CLI Reference

### Starting Krill

```bash
# Start daemon and open TUI
krill up recipe.yaml

# Start daemon only
krill up recipe.yaml -d

# Connect to running daemon
krill
```

### Managing Services

```bash
# View logs
krill logs service-name

# Restart service
krill restart service-name

# Stop service
krill stop service-name

# Stop all services and daemon
krill down
```

### TUI Keybindings

| Key | Action |
|-----|--------|
| ↑/k | Previous service |
| ↓/j | Next service |
| Enter | View logs |
| d | Detail view |
| r | Restart service |
| s | Stop service |
| S | Stop daemon |
| q | Quit TUI |
| h | Help |

## Configuration Schema

Krill validates your recipes against a JSON schema. See the [schema file](../schemas/krill.schema.json) for complete validation rules.

## Troubleshooting

### Common Issues

**Service won't start:**
- Check dependencies are satisfied
- Verify execute configuration is correct
- Review service logs with `krill logs service-name`

**Health check always fails:**
- Verify port/endpoint is correct
- Check service is actually listening
- Review health check timeout

**Circular dependency error:**
- Review dependency graph for cycles
- Redesign to break circular references

**Daemon won't start:**
- Check for syntax errors in recipe
- Verify all service names are valid
- Review daemon logs in `~/.krill/logs/`

### Getting Help

- Check the [Configuration Guide](configuration.md) for syntax
- Review [Health Checks Guide](health-checks.md) for monitoring
- See [Dependencies Guide](dependencies.md) for orchestration
- Report issues at https://github.com/anthropics/krill/issues

## Advanced Topics

### Safety Features

- **Critical services**: Emergency stop on failure
- **GPU validation**: Check GPU availability before starting
- **Shell safety**: Automatic validation of shell commands
- **PGID isolation**: Each service in its own process group

### Logging

- Per-service logs: `~/.krill/logs/session-*/service-name.log`
- Timeline aggregation: `~/.krill/logs/session-*/timeline.jsonl`
- Daemon logs: `~/.krill/logs/session-*/krill.log`

### IPC Protocol

Krill uses a JSON-based IPC protocol over Unix sockets for client-daemon communication. See the source code for protocol details.

## Contributing

Krill is open-source under the Apache-2.0 license. Contributions welcome!

See [CONTRIBUTING.md](../CONTRIBUTING.md) for guidelines.

## License

Apache-2.0

---

**Need more help?** Check the [Quick Reference](quick-reference.md) for common patterns or the [Configuration Guide](configuration.md) for complete syntax.

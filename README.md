# Krill - Process orchestrator for Robotics

<p align="center">
  <img src="/assets/banner-krill.png" alt="Krill-banner" width="220">
</p>

**Professional-grade process orchestrator for robotics systems** built in Rust.

[![Build Status](https://img.shields.io/badge/build-passing-brightgreen)]()
[![Tests](https://img.shields.io/badge/tests-155%20passing-brightgreen)]()
[![License](https://img.shields.io/badge/license-Apache--2.0-blue)]()

## Overview

Krill provides DAG-based service orchestration, health monitoring, and safety interception for critical robotics applications. It manages complex dependency graphs of services (pixi tasks, ROS2 launch files, shell commands) with automatic restart policies, fault cascading, and emergency stop capabilities.

**Key Features:**

- ⚡ **DAG-based orchestration** - Services start/stop in correct dependency order
- 🔄 **Automatic restarts** - Configurable policies: always, on-failure, never
- 💚 **Health monitoring** - Heartbeat, TCP, HTTP, and script-based checks
- 🚨 **Safety interception** - Critical service failures trigger emergency stop
- 🔗 **Cascading failures** - Dependent services stop when dependencies fail
- 📊 **Terminal UI** - Monitoring interface
- 🔌 **IPC protocol** - JSON-based client-server communication
- 📝 **Session logging** - Per-service logs with timeline aggregation
- 🎮 **GPU validation** - Checks GPU availability before starting services
- 🛡️ **Shell safety** - Validates and rejects dangerous shell patterns

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                        Krill Daemon                         │
│  ┌────────────────┐  ┌────────────────┐  ┌───────────────┐  │
│  │  Orchestrator  │──│ Service Runner │──│ Health Monitor│  │
│  └────────────────┘  └────────────────┘  └───────────────┘  │
│  ┌────────────────┐  ┌────────────────┐                     │
│  │   IPC Server   │──│   Log Manager  │                     │
│  └────────────────┘  └────────────────┘                     │
└───────────┬─────────────────────────────────────────────────┘
            │ Unix Socket (/tmp/krill.sock)
    ┌───────┴────────┬────────────────┬──────────────┬───────────────┐
    │                │                │              │               │
┌───▼────┐    ┌──────▼─────┐   ┌─────▼─────┐   ┌─────▼─────┐   ┌─────▼─────┐
│  TUI   │    │ Rust SDK   │   │  C++ SDK  │   │  Service  │   │  CLI      │
└────────┘    └────────────┘   └───────────┘   └───────────┘   └───────────┘
```

## Quick Start

### Installation

TODO

### Running the Daemon

TODO

### Using the TUI

```bash
# Connect to the running daemon
krill-tui

# Or with custom socket
krill-tui --socket /var/run/krill.sock
```

### Configuration Example

```yaml
version: "1"
name: robot
log_dir: ~/.krill/logs

env:
  ROBOT_ID: robot-001
  ROS_DOMAIN_ID: "42"

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
    policy:
      restart: always

  vision:
    execute:
      type: pixi
      task: vision-pipeline
    gpu: true
    health_check:
      type: heartbeat
      timeout: 1s
```

## SDKs

### Rust SDK

```rust
use krill_sdk_rust::KrillClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = KrillClient::new("my-service").await?;
    
    loop {
        // Do work...
        
        // Send heartbeat
        client.heartbeat().await?;
        
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
}
```

### Python SDK

```python
from krill import KrillClient

with KrillClient("my-service") as client:
    while True:
        # Do work...
        
        # Send heartbeat
        client.heartbeat()
        
        time.sleep(1)
```

**Async version:**

```python
import asyncio
from krill import AsyncKrillClient

async def main():
    client = await AsyncKrillClient.connect("my-service")
    
    while True:
        # Do work...
        
        # Send heartbeat
        await client.heartbeat()
        
        await asyncio.sleep(1)

asyncio.run(main())
```

### C++ SDK (Header-only)

```cpp
#include "krill.hpp"

int main() {
    try {
        krill::Client client("my-service");
        
        while (true) {
            // Do work...
            
            // Send heartbeat
            client.heartbeat();
            
            std::this_thread::sleep_for(std::chrono::seconds(1));
        }
    } catch (const krill::KrillError& e) {
        std::cerr << "Error: " << e.what() << std::endl;
        return 1;
    }
}
```

## Execute Types

Krill supports multiple execution backends:

- **Pixi** - Python package manager tasks
- **ROS2** - ROS2 launch files with arguments
- **Shell** - Safe shell commands (validated)
- **Docker** - Container execution (requires Krill Pro)

## Health Checks

- **Heartbeat** - Services send periodic heartbeats via SDK
- **TCP** - Port connectivity checks
- **HTTP** - HTTP endpoint with status code validation
- **Script** - Custom health check commands

## TUI Key Bindings

| Key | Action |
|-----|--------|
| ↑/k | Previous service |
| ↓/j | Next service |
| Enter | View logs |
| d | Detail view |
| r | Restart service |
| s | Stop service |
| S | Stop daemon (with confirmation) |
| q | Quit TUI |

## Project Structure

```
krill/
├── crates/
│   ├── krill-common/      # Shared types and protocols
│   ├── krill-daemon/      # Daemon orchestrator
│   ├── krill-tui/         # Terminal UI
│   ├── krill-sdk-rust/    # Rust client SDK
│   └── krill-cli/         # Unified CLI
├── sdk/
│   ├── krill-cpp/         # C++ header-only SDK
│   └── krill-python/      # Python SDK (sync + async)
├── schemas/
│   └── krill.schema.json  # JSON schema for configs
├── examples/
│   └── krill.yaml         # Example configuration
└── tests/
    └── integration/       # Integration tests
```

## Development

### Prerequisites

- Rust 1.70+ (edition 2021)
- tokio async runtime
- Unix-like OS (Linux, macOS)

### Build Commands

```bash
# Build all
just build

# Run tests
just test

# Run linter
just lint

# Format code
just fmt

# Run all checks
just check

# Build documentation
just doc
```

### Testing

```bash
# Run all tests
cargo test --workspace

# Run with output
cargo test --workspace -- --nocapture

# Run specific test
cargo test --package krill-common config::
```

## Safety & Validation

- **Shell command validation** - Rejects pipes, redirections, command substitution
- **PGID isolation** - Each service in its own process group
- **GPU validation** - Checks GPU availability before starting
- **Dependency validation** - Ensures all dependencies exist
- **Config validation** - Validates YAML against schema

## Performance

- **Lightweight** - Minimal overhead per service
- **Async I/O** - Non-blocking event-driven architecture
- **Concurrent startup** - Services in same DAG layer start in parallel
- **Efficient logging** - Buffered writes, optional log rotation

## License

Apache-2.0

## Open-Core Philosophy

Krill follows an **open-core model**. The community edition you see here is fully open-source under the Apache-2.0 license and covers everything needed to orchestrate robotics services in production:

- DAG-based orchestration, health monitoring, restart policies, cascading failures, and safety interception
- Terminal UI, CLI, and client SDKs (Rust, Python, C++)
- Pixi, ROS2, and shell execution backends

**Krill Pro** (coming soon) extends the core with enterprise features for larger teams and fleet deployments:

- Docker execution backend
- Advanced scheduling policies
- Fleet-wide orchestration and remote management
- Metrics export and observability integrations
- Priority support

The boundary is simple: if you're running services on a single robot or dev machine, the open-source edition has you covered. Pro targets multi-robot fleets and enterprise operational needs.

We believe the core orchestrator should always be free and community-driven. Revenue from Pro funds continued development of both editions.

## Roadmap

- [x] Phase 0: Workspace setup
- [x] Phase 1-2: Shared types and DAG resolution
- [x] Phase 3-4: Process management and health checks
- [x] Phase 5-6: Daemon orchestrator, IPC, and logging
- [x] Phase 7: Terminal UI
- [x] Phase 8: SDKs (Rust + C++ + Python)
- [ ] Phase 9: Advanced features (log rotation, metrics export, always_alive)
- [ ] Phase 10: Docker support (Docker support)
- [ ] Phase 11: Docker pro (advanced policies)

## Acknowledgments

Built with Rust and the following excellent crates:
- tokio - Async runtime
- ratatui - Terminal UI framework
- serde - Serialization framework
- clap - CLI argument parsing
- nix - Unix system interfaces

# Krill - Process orchestrator for Robotics

<p align="center">
  <img src="/assets/banner-krill.png" alt="Krill-banner" width="320">
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


## Quick Start

### Installation

```bash
just install
```


### Create a recipe
```yaml
services:
  - name: my-service
    image: my-image
    ports:
      - 8080:8080
```

### Running krill

Start the daemon and open the TUI
```bash
krill up krill.yaml
```
you can skip opening the TUI with the option `-d/--daemon`.

If a daemon is already running, just connect to the TUI
```bash
krill
```

Stop krill with the command:
```bash
krill down
```
## How it looks

add demo video here
<https://github.com/krill-robotics/krill-demo>


## Why krill?

After working on various robotics projects, we realised the need for a robust process orchestrator that could handle complex dependencies and provide a user-friendly interface for monitoring and managing services. Krill was born out of this need, with a focus on:
- **Predictability**: Know exactly why a service failed and which dependent nodes were brought down as a result.
- **Safety-First**: If a critical "Guardian" node fails, Krill can trigger an immediate system-wide shutdown or emergency state.
- **Tool Agnostic**: Stop fighting environment variables. Seamlessly mix Rust, Python, C++, and Dockerized workloads in a single graph.


### Execute types & Health checks

**Backends**:
- **Pixi** - Python package manager tasks (Highly recommended).
- **ROS2** - Launch files with argument support.
- **Docker** - Containerized execution.
- **Shell** - Validated safe shell commands.

**Health Checks**
- **Heartbeat** - Services "check-in" via SDK (Rust/Python/C++).
- **TCP/HTTP** - Port and endpoint validation.
- **Script** - Run a custom command to verify health.

## SDKs
Krill provides SDKs for Rust, Python, and C++ to facilitate easy integration with your services.

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
## Open-Core Philosophy

Krill follows an **open-core model**. The community edition you see here is fully open-source under the Apache-2.0 license and covers everything needed to orchestrate robotics services in production:

- DAG-based orchestration, health monitoring, restart policies, cascading failures, and safety interception
- Terminal UI, CLI, and client SDKs (Rust, Python, C++)
- Pixi, ROS2, Docker, and shell execution backends

**Krill Pro** (coming soon) extends the core with enterprise features for larger teams and fleet deployments:

- Advanced scheduling policies
- Fleet-wide orchestration and remote management
- Metrics export and observability integrations
- Priority support

The boundary is simple: if you're running services on a single robot or dev machine, the open-source edition has you covered. Pro targets multi-robot fleets and enterprise operational needs.

We believe the core orchestrator should always be free and community-driven. Revenue from Pro funds continued development of both editions.



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
| h | Help |

## Development & Safety

Building from source:
```bash
just check
just build
```
**Safety Design**

- **Shell command validation** - Rejects pipes, redirections, command substitution
- **PGID isolation** - Each service in its own process group
- **GPU validation** - Checks GPU availability before starting
- **Dependency validation** - Ensures all dependencies exist
- **Config validation** - Validates YAML against schema

## License

Apache-2.0

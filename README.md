# Krill - Process orchestrator for Robotics

<p align="center">
  <img src="/docs/assets/banner-krill.png" alt="Krill-banner" width="320">
</p>

**Professional-grade process orchestrator for robotics systems** built in Rust.

Unlike ROS2 launch or Docker compose, Krill adds safety-first orchestration with cascading failures, critical service protection, and a real-time monitoring UI designed for robots.

[![Build Status](https://img.shields.io/badge/build-passing-brightgreen)]()
[![Documentation](https://img.shields.io/badge/docs-mkdocs-blue)](https://zero-robotics.github.io/krill/)
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

Here's a complete example orchestrating a ROS2 robot navigation stack:

```yaml
version: "1"
name: autonomous-robot
log_dir: ~/.krill/logs

env:
  ROS_DOMAIN_ID: "42"
  ROS_LOCALHOST_ONLY: "0"

services:
  # Hardware drivers start first
  lidar:
    execute:
      type: ros2
      package: ldlidar_ros2
      launch_file: ldlidar.launch.py
    health_check:
      type: tcp
      port: 4048
    policy:
      restart: on-failure
      max_restarts: 3

  camera:
    execute:
      type: ros2
      package: realsense2_camera
      launch_file: rs_launch.py
      launch_args:
        enable_depth: "true"
        enable_color: "true"
    dependencies:
      - lidar
    health_check:
      type: tcp
      port: 8554

  # SLAM for mapping and localization
  slam:
    execute:
      type: ros2
      package: slam_toolbox
      launch_file: online_async_launch.py
    dependencies:
      - lidar: healthy
      - camera: healthy
    health_check:
      type: heartbeat
      timeout: 5s

  # Navigation stack
  navigation:
    execute:
      type: ros2
      package: nav2_bringup
      launch_file: navigation_launch.py
    dependencies:
      - slam: healthy
    critical: true  # If navigation fails, stop everything
    health_check:
      type: http
      port: 8080
      path: /health
    policy:
      restart: always
      restart_delay: 2s

  # Web dashboard
  dashboard:
    execute:
      type: docker
      image: ghcr.io/robotics/web-ui:latest
      ports:
        - "3000:3000"
      volumes:
        - "./config:/app/config:ro"
      network: host
    dependencies:
      - navigation: started
```

See [Configuration Guide](https://zero-robotics.github.io/krill/configuration/) for all available options.

### Running krill

Start the daemon and open the TUI
```bash
krill up krill.yaml
```

If a daemon is already running, just connect to the TUI
```bash
krill
```

Stop krill with the command:
```bash
krill down
```
## Why krill?

After working on various robotics projects, we realised the need for a robust process orchestrator that could handle complex dependencies and provide a user-friendly interface for monitoring and managing services. Krill was born out of this need, with a focus on:
- **Predictability**: Know exactly why a service failed and which dependent nodes were brought down as a result.
- **Safety-First**: If a critical "Guardian" node fails, Krill can trigger an immediate system-wide shutdown or emergency state.
- **Tool Agnostic**: Stop fighting environment variables. Seamlessly mix Rust, Python, C++, and Dockerized workloads in a single graph.


## How it looks
https://github.com/user-attachments/assets/4707d2e5-42ac-4d92-8fba-749ccb340a2c

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


## Learn more
[Full Documentation](https://zero-robotics.github.io/krill/)
[Examples](https://zero-robotics.github.io/krill/examples/)
[SDK Installation](https://zero-robotics.github.io/krill/sdk-installation/)

## License

Apache-2.0

Copyright 2026 Tommaso Pardi

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.

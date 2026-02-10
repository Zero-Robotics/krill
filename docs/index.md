---
title: Krill
hide:
  - toc
  - title
---

<div align="center">
  <img src="assets/banner-krill.png" alt="Krill Logo" width="500"/>
</div>
  **Modern DAG orchestrator for robotics**
  
  [![GitHub stars](https://img.shields.io/github/stars/Zero-Robotics/krill?style=social)](https://github.com/Zero-Robotics/krill)
  [![Docs](https://img.shields.io/badge/docs-passing-brightgreen)](https://Zero-Robotics.github.io/krill)


## Why Krill?

!!! tip "Perfect for Robotics"
    Krill is designed specifically for robotics workloads with ROS2, Pixi and Docker integration, together with real-time monitoring. It provides a seamless experience for managing complex robotics systems.

Build on Rust 🦀
<div class="performance-metric">
  <strong>⚡ lightning-fast</strong> performance
</div>
<div class="performance-metric">
  <strong>🔒 Safety Critical </strong> performance
</div>

## Features

<div class="grid cards" markdown>

-   :material-lightning-bolt:{ .lg .middle } __TUI integration__

    ---

    Rich text user interface (TUI) for managing and monitoring processes. The TUI offers a clear and intuitive way to view and interact with your robotics system

-   :material-robot:{ .lg .middle } __ROS2 Native__

    ---

    First-class support for ROS2 packages, and launchers. Either natively supported, via Docker, or via Pixi

-   :material-graph:{ .lg .middle } __DAG Orchestration__

    ---

    Manage complex process dependencies with ease

-   :material-heart-pulse:{ .lg .middle } __Health Monitoring__

    ---

    Real-time process health checks and automatic recovery

</div>

## Quick Start

=== "**Prerequisites**"
  
  -   [Just](https://just.systems/) installed
  -   [Cargo](https://doc.rust-lang.org/cargo/getting-started/installation.html) installed
  -   [Pixi](https://pixi.rs/) installed (not mandatory, but usefulfor running examples)
  
=== "**Install**"
  
  ```bash
  just install
  ```
  
=== "**Configure**"
  
  ```yaml title="krill.yaml"
  services:
    ros_talker:
      execute:
        type: pixi
        task: talker
        working_dir: ./ros2
      dependencies: []
    ros_listener:
      execute:
        type: pixi
        task: listener
        working_dir: ./ros2
      dependencies:
        - ros_talker
  ```
  
=== "**Run**"
  
  ```bash
  krill up krill.yaml
  ```


## Next Steps

<div class="grid cards" markdown>

-   [Getting Started →](getting-started.md)
-   [Configuration Guide →](configuration.md)
-   [ROS2 Integration →](ros2/overview.md)
-   [API Reference →](api/index.md)

</div>

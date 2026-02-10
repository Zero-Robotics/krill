---
title: Krill
hide:
  - toc
  - title
---

<div align="center">
  <img src="assets/banner-krill.png" alt="Krill Logo" width="500"/>
</div>
  **Modern DAG orchestrator for robotics: Build on Rust 🦀**
  
  [![GitHub stars](https://img.shields.io/github/stars/Zero-Robotics/krill?style=social)](https://github.com/Zero-Robotics/krill)
  [![Docs](https://img.shields.io/badge/docs-passing-brightgreen)](https://Zero-Robotics.github.io/krill)

## Why Krill?

!!! tip "Built for Robotics"
    Krill is an orchestrator designed specifically for robotics, built to manage the real-world complexity of modern robotic systems.
    
    ROS / ROS 2 has become a de-facto standard, and Docker is increasingly adopted by roboticists.  
    But once systems grow beyond a single machine, mixing native processes, containers, hardware drivers, and launch logic quickly becomes brittle.
    
    Krill bridges this gap by providing a unified way to **compose, run, and operate** complex robotics stacks — from laptops to production robots.

!!! success "What Krill Solves"

    **One tool. Your entire robotics stack.**
    
    - **ROS 2 Native**  
      First-class support for packages, launch files, lifecycle nodes, and ROS-centric workflows
    
    - **Docker & Pixi Ready**  
      Seamlessly mix containerized, virtual-env, and native workloads in a single system
    
    - **Smart Orchestration**  
      DAG-based dependency management ensures deterministic startup, shutdown, and recovery
    
    - **Production-Grade Observability**  
      Built-in health checks, status propagation, and automatic restart semantics
    
    - **Policy-Driven Operation**  
      Encode operational constraints, safety rules, and system invariants as policies — not scripts
    
    From local development to production robots — one configuration, zero rewrites.

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
-   [Examples →](examples/index.md)
-   [Quick Reference →](quick-reference.md)

</div>

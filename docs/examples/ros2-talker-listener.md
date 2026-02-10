# ROS2 Talker / Listener

A minimal ROS2 example using Pixi to run the standard `demo_nodes_cpp` talker and listener.

<!-- Video placeholder -->

## Recipe

```yaml title="examples/krill-ros2.yaml"
version: "1"
name: robot
log_dir: ~/.krill/logs

env:
  ROBOT_ID: robot-001
  ROS_DOMAIN_ID: "42"

services:
  ros_talker:
    execute:
      type: pixi
      task: talker
      working_dir: ./ros2
    dependencies: []
    critical: true
    policy:
      restart: always
      max_restarts: 3
      restart_delay: 5s

  ros_listener:
    execute:
      type: pixi
      task: listener
      working_dir: ./ros2
    dependencies:
      - ros_talker
    policy:
      restart: always
      max_restarts: 3
      restart_delay: 5s
```

The `examples/ros2/pixi.toml` defines the tasks using the `robostack-jazzy` channel:

```toml title="examples/ros2/pixi.toml"
[project]
name = "krill-ros2-example"
channels = ["robostack-jazzy", "conda-forge"]
platforms = ["linux-64", "osx-arm64"]

[dependencies]
ros-jazzy-demo-nodes-cpp = "*"

[tasks]
talker = "ros2 run demo_nodes_cpp talker"
listener = "ros2 run demo_nodes_cpp listener"
```

## Running

```bash
cd examples/ros2 && pixi install && cd ../..
krill up examples/krill-ros2.yaml
```

## Key Concepts

- **Pixi + ROS2** — Pixi manages the ROS2 environment so you don't need a system-wide ROS install.
- **Dependency ordering** — the listener waits for the talker to start before launching.
- **`critical: true`** — if the talker dies permanently, the whole stack stops.

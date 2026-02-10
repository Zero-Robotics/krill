# ROS2 Navigation Stack

A full autonomous robot stack: lidar, camera, SLAM, navigation, and a web dashboard.

<!-- Video placeholder -->

## Architecture

```
lidar ──────┐
            ├──▶ slam ──▶ navigation (critical) ──▶ dashboard
camera ─────┘
```

## Recipe

```yaml title="examples/krill-ros2-navigation.yaml"
version: "1"
name: autonomous-robot
log_dir: ~/.krill/logs

env:
  ROS_DOMAIN_ID: "42"
  ROS_LOCALHOST_ONLY: "0"

services:
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

  navigation:
    execute:
      type: ros2
      package: nav2_bringup
      launch_file: navigation_launch.py
    dependencies:
      - slam: healthy
    critical: true
    health_check:
      type: http
      port: 8080
      path: /health
    policy:
      restart: always
      restart_delay: 2s

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

## Key Concepts

- **Mixed executors** — ROS2 launch files alongside a Docker container in the same recipe.
- **`type: ros2`** — launches ROS2 packages directly with `launch_args`.
- **Health check variety** — TCP for hardware drivers, heartbeat for SLAM, HTTP for navigation.
- **`critical: true`** — navigation failure triggers emergency stop of the entire stack.
- **`network: host`** — the dashboard container shares the host network to reach ROS2 topics.

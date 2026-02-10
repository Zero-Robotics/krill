# Krill Configuration Guide

Complete reference for Krill recipe configuration files.

## Table of Contents

- [File Structure](#file-structure)
- [Top-Level Fields](#top-level-fields)
- [Service Configuration](#service-configuration)
- [Execute Types](#execute-types)
- [Health Checks](#health-checks)
- [Policies](#policies)
- [Dependencies](#dependencies)
- [Complete Example](#complete-example)

## File Structure

Krill uses YAML configuration files (recipes) to define your service orchestration:

```yaml
version: "1"
name: my-workspace
log_dir: ~/.krill/logs
env:
  KEY: value

services:
  service-name:
    execute: {...}
    dependencies: [...]
    health_check: {...}
    policy: {...}
    critical: false
    gpu: false
```

## Top-Level Fields

### `version` (required)

**Type:** `string`  
**Value:** `"1"`

Schema version. Currently only version `"1"` is supported.

```yaml
version: "1"
```

### `name` (required)

**Type:** `string`  
**Pattern:** `^[a-zA-Z0-9_-]+$`

Workspace name used in process naming. Must contain only alphanumeric characters, hyphens, and underscores.

```yaml
name: autonomous-robot
```

### `log_dir` (optional)

**Type:** `string`  
**Default:** System default location

Directory for log files. Supports tilde (`~`) expansion.

```yaml
log_dir: ~/.krill/logs
```

### `env` (optional)

**Type:** `object`  
**Default:** `{}`

Global environment variables applied to all services.

```yaml
env:
  ROS_DOMAIN_ID: "42"
  LOG_LEVEL: "info"
  PYTHONUNBUFFERED: "1"
```

## Service Configuration

Each service is defined under the `services` key with a unique name.

### Service Name

**Pattern:** `^[a-zA-Z0-9_-]+$`

Must contain only alphanumeric characters, hyphens, and underscores.

```yaml
services:
  my-service:  # Valid
    # ...
  
  camera_0:    # Valid
    # ...
```

### Service Fields

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `execute` | [ExecuteConfig](#execute-types) | Yes | - | How to run the service |
| `dependencies` | [Dependency[]](#dependencies) | No | `[]` | Services this depends on |
| `health_check` | [HealthCheck](#health-checks) | No | `null` | Health monitoring config |
| `policy` | [Policy](#policies) | No | See [Policies](#policies) | Restart and timeout settings |
| `critical` | `boolean` | No | `false` | Trigger emergency stop on failure |
| `gpu` | `boolean` | No | `false` | Validate GPU availability before start |

#### Example Service

```yaml
services:
  navigation:
    execute:
      type: ros2
      package: nav2_bringup
      launch_file: navigation_launch.py
    dependencies:
      - slam: healthy
    critical: true
    gpu: false
    health_check:
      type: http
      port: 8080
      path: /health
    policy:
      restart: always
      max_restarts: 5
      restart_delay: 2s
      stop_timeout: 10s
```

## Execute Types

The `execute` field defines how a service runs. Four types are supported:

### Pixi

Runs tasks from Pixi (Python package manager).

**Fields:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | `"pixi"` | Yes | Execute type |
| `task` | `string` | Yes | Pixi task name |
| `environment` | `string` | No | Pixi environment (defaults to service name) |
| `stop_task` | `string` | No | Graceful stop task |
| `working_dir` | `string` | No | Working directory |

**Example:**

```yaml
execute:
  type: pixi
  task: start-server
  environment: production
  stop_task: cleanup
  working_dir: ./services/api
```

### ROS2

Launches ROS2 packages.

**Fields:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | `"ros2"` | Yes | Execute type |
| `package` | `string` | Yes | ROS2 package name |
| `launch_file` | `string` | Yes | Launch file name |
| `launch_args` | `object` | No | Launch arguments (key-value pairs) |
| `stop_task` | `string` | No | Optional Pixi stop task |
| `working_dir` | `string` | No | Working directory |

**Example:**

```yaml
execute:
  type: ros2
  package: nav2_bringup
  launch_file: navigation_launch.py
  launch_args:
    use_sim_time: "false"
    map: "/maps/warehouse.yaml"
    params_file: "config/nav2_params.yaml"
```

### Docker

Runs containerized services.

**Fields:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | `"docker"` | Yes | Execute type |
| `image` | `string` | Yes | Docker image name |
| `volumes` | `string[]` | No | Volume mounts (`host:container` or `host:container:ro`) |
| `ports` | `string[]` | No | Port mappings (`host:container`) |
| `privileged` | `boolean` | No | Run in privileged mode (default: `false`) |
| `network` | `string` | No | Network mode (`bridge`, `host`, etc.) |

**Example:**

```yaml
execute:
  type: docker
  image: ghcr.io/robotics/perception:v2.1
  volumes:
    - "/dev/video0:/dev/video0"
    - "./models:/app/models:ro"
  ports:
    - "8080:8080"
    - "9090:9090"
  privileged: true
  network: host
```

### Shell

Executes validated shell commands.

**Fields:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | `"shell"` | Yes | Execute type |
| `command` | `string` | Yes | Shell command (validated for safety) |
| `stop_command` | `string` | No | Graceful stop command |
| `working_dir` | `string` | No | Working directory |

**Safety:** Shell commands are validated and reject pipes (`|`), redirections (`>`, `<`), command substitution (`` ` ``), and other dangerous patterns.

**Example:**

```yaml
execute:
  type: shell
  command: python -m http.server 8000
  stop_command: pkill -f "http.server 8000"
  working_dir: ./public
```

## Health Checks

Health checks monitor service status and determine when a service is "healthy".

### Heartbeat

Services actively report their health via SDK.

**Fields:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | `"heartbeat"` | Yes | Health check type |
| `timeout` | `string` | Yes | Max time between heartbeats (e.g., `2s`, `500ms`) |

**Example:**

```yaml
health_check:
  type: heartbeat
  timeout: 5s
```

**SDK Usage:**

```python
from krill import KrillClient

with KrillClient("my-service") as client:
    while True:
        # Do work...
        client.heartbeat()
        time.sleep(1)
```

### TCP

Checks if a TCP port is open.

**Fields:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | `"tcp"` | Yes | Health check type |
| `port` | `integer` | Yes | TCP port (1-65535) |
| `timeout` | `string` | No | Connection timeout |

**Example:**

```yaml
health_check:
  type: tcp
  port: 8080
  timeout: 2s
```

### HTTP

Performs HTTP health checks.

**Fields:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | `"http"` | Yes | Health check type |
| `port` | `integer` | Yes | HTTP port (1-65535) |
| `path` | `string` | No | Endpoint path (default: `/health`) |
| `expected_status` | `integer` | No | Expected HTTP status (default: `200`) |

**Example:**

```yaml
health_check:
  type: http
  port: 8080
  path: /api/health
  expected_status: 200
```

### Script

Runs a custom command to verify health.

**Fields:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | `"script"` | Yes | Health check type |
| `command` | `string` | Yes | Health check command (exit 0 = healthy) |
| `timeout` | `string` | No | Execution timeout |

**Example:**

```yaml
health_check:
  type: script
  command: curl -f http://localhost:8080/health
  timeout: 3s
```

## Policies

Control restart behavior and timeouts.

### Policy Fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `restart` | `"never"` \| `"always"` \| `"on-failure"` | `"never"` | When to restart |
| `max_restarts` | `integer` | `0` | Max restart attempts (0 = unlimited) |
| `restart_delay` | `string` | `"1s"` | Delay between restarts |
| `stop_timeout` | `string` | `"10s"` | Timeout before SIGKILL |

### Restart Policies

- **`never`**: Never restart the service automatically
- **`always`**: Always restart, regardless of exit code
- **`on-failure`**: Only restart if the service exits with non-zero code

**Example:**

```yaml
policy:
  restart: on-failure
  max_restarts: 3
  restart_delay: 5s
  stop_timeout: 30s
```

## Dependencies

Services can depend on other services with different conditions.

### Simple Dependency

Wait for service to start (doesn't check health):

```yaml
dependencies:
  - lidar
  - camera
```

### Conditional Dependency

Wait for specific conditions:

```yaml
dependencies:
  - lidar: started   # Wait until started
  - camera: healthy  # Wait until healthy
```

### Dependency Conditions

- **`started`**: Service has been started (default)
- **`healthy`**: Service is running AND health check passes

**Example:**

```yaml
services:
  sensors:
    execute:
      type: ros2
      package: sensor_drivers
      launch_file: sensors.launch.py

  processing:
    execute:
      type: pixi
      task: run-processor
    dependencies:
      - sensors: healthy  # Wait for sensors to be healthy

  visualization:
    execute:
      type: docker
      image: viz:latest
    dependencies:
      - sensors: started   # Just wait for sensors to start
      - processing: healthy # Wait for processing to be healthy
```

## Duration Format

Many fields accept duration strings with these units:

- `ms` - milliseconds
- `s` - seconds
- `m` - minutes
- `h` - hours

**Examples:**
- `500ms` - 500 milliseconds
- `5s` - 5 seconds
- `2m` - 2 minutes
- `1h` - 1 hour

## Complete Example

```yaml
version: "1"
name: mobile-robot
log_dir: ~/.krill/logs

env:
  ROS_DOMAIN_ID: "42"
  LOG_LEVEL: "info"

services:
  # Hardware layer
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
      restart_delay: 2s

  # Perception layer
  slam:
    execute:
      type: pixi
      task: run-slam
      environment: perception
    dependencies:
      - lidar: healthy
    gpu: true
    health_check:
      type: heartbeat
      timeout: 5s
    policy:
      restart: always

  # Control layer
  navigation:
    execute:
      type: ros2
      package: nav2_bringup
      launch_file: navigation_launch.py
      launch_args:
        use_sim_time: "false"
    dependencies:
      - slam: healthy
    critical: true
    health_check:
      type: http
      port: 8080
      path: /health

  # Monitoring
  dashboard:
    execute:
      type: docker
      image: grafana/grafana:latest
      ports:
        - "3000:3000"
      volumes:
        - "./grafana-data:/var/lib/grafana"
      network: host
    dependencies:
      - navigation: started
```

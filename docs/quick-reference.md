# Quick Reference

Fast lookup for common Krill configurations.

## Minimal Recipe

```yaml
version: "1"
name: my-project

services:
  my-service:
    execute:
      type: shell
      command: python server.py
```

## Execute Types

### Pixi

```yaml
execute:
  type: pixi
  task: start-server
  environment: production  # optional
  working_dir: ./services  # optional
  stop_task: cleanup       # optional
```

### ROS2

```yaml
execute:
  type: ros2
  package: nav2_bringup
  launch_file: navigation_launch.py
  launch_args:              # optional
    use_sim_time: "false"
    map: "/maps/map.yaml"
  working_dir: ./ros2_ws    # optional
```

### Docker

```yaml
execute:
  type: docker
  image: nginx:latest
  volumes:                  # optional
    - "/host:/container"
    - "/data:/app:ro"
  ports:                    # optional
    - "8080:80"
    - "443:443"
  privileged: false         # optional
  network: bridge           # optional
```

### Shell

```yaml
execute:
  type: shell
  command: npm start
  working_dir: ./app        # optional
  stop_command: npm stop    # optional
```

## Health Checks

### Heartbeat

```yaml
health_check:
  type: heartbeat
  timeout: 5s
```

### TCP

```yaml
health_check:
  type: tcp
  port: 8080
  timeout: 2s  # optional
```

### HTTP

```yaml
health_check:
  type: http
  port: 8080
  path: /health              # optional, default: /health
  expected_status: 200       # optional, default: 200
```

### Script

```yaml
health_check:
  type: script
  command: curl -f localhost:8080/health
  timeout: 3s  # optional
```

## Dependencies

### Simple (wait for start)

```yaml
dependencies:
  - service-a
  - service-b
```

### Conditional (wait for healthy)

```yaml
dependencies:
  - service-a: started
  - service-b: healthy
```

## Policies

### Never Restart

```yaml
policy:
  restart: never
```

### Always Restart

```yaml
policy:
  restart: always
  max_restarts: 0      # unlimited
  restart_delay: 1s
  stop_timeout: 10s
```

### Restart on Failure

```yaml
policy:
  restart: on-failure
  max_restarts: 3
  restart_delay: 5s
  stop_timeout: 30s
```

## Common Flags

### Critical Service

```yaml
services:
  safety-monitor:
    critical: true  # Emergency stop on failure
    # ...
```

### GPU Required

```yaml
services:
  ml-inference:
    gpu: true  # Validate GPU before starting
    # ...
```

## Complete Examples

### Simple Web App

```yaml
version: "1"
name: webapp

services:
  backend:
    execute:
      type: docker
      image: api:latest
      ports:
        - "8000:8000"
    health_check:
      type: http
      port: 8000
      path: /health
    policy:
      restart: always
```

### ROS2 Robot

```yaml
version: "1"
name: robot

env:
  ROS_DOMAIN_ID: "42"

services:
  lidar:
    execute:
      type: ros2
      package: ldlidar_ros2
      launch_file: ldlidar.launch.py
    health_check:
      type: tcp
      port: 4048

  navigation:
    execute:
      type: ros2
      package: nav2_bringup
      launch_file: navigation_launch.py
    dependencies:
      - lidar: healthy
    critical: true
```

### Microservices

```yaml
version: "1"
name: microservices

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
      task: start
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

## Duration Format

| Unit | Example | Description |
|------|---------|-------------|
| `ms` | `500ms` | Milliseconds |
| `s`  | `5s`    | Seconds |
| `m`  | `2m`    | Minutes |
| `h`  | `1h`    | Hours |

## CLI Commands

```bash
# Start daemon and open TUI
krill up recipe.yaml

# Start daemon only (no TUI)
krill up recipe.yaml -d

# Connect to running daemon
krill

# Stop all services and daemon
krill down

# View logs
krill logs service-name

# Restart service
krill restart service-name

# Stop service
krill stop service-name
```

## TUI Keybindings

| Key | Action |
|-----|--------|
| `↑`/`k` | Previous service |
| `↓`/`j` | Next service |
| `Enter` | View logs |
| `d` | Detail view |
| `r` | Restart service |
| `s` | Stop service |
| `S` | Stop daemon |
| `q` | Quit TUI |
| `h` | Help |

## Validation Rules

### Service Names

- Only alphanumeric, hyphens, underscores
- Cannot be empty
- Examples: `my-service`, `camera_0`, `lidar1`

### Shell Commands

**Rejected patterns:**
- Pipes: `|`
- Redirections: `>`, `<`, `>>`
- Command substitution: `` ` ``, `$()`
- Background: `&`
- Command chaining: `;`, `&&`, `||`

**Safe commands:**
```yaml
command: python server.py           # ✅
command: node app.js --port 3000   # ✅
command: ls | grep foo             # ❌
command: echo "hi" > output.txt    # ❌
```

## See Also

- [Configuration Guide](configuration.md) - Complete reference
- [Health Checks Guide](health-checks.md) - Health check patterns
- [Dependencies Guide](dependencies.md) - Orchestration patterns

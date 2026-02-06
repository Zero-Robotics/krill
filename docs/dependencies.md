# Dependencies and DAG Orchestration

Master service dependencies and build reliable orchestration graphs with Krill.

## Table of Contents

- [Overview](#overview)
- [Dependency Basics](#dependency-basics)
- [Dependency Conditions](#dependency-conditions)
- [Startup Order](#startup-order)
- [Failure Handling](#failure-handling)
- [Common Patterns](#common-patterns)
- [Best Practices](#best-practices)

## Overview

Krill uses a **Directed Acyclic Graph (DAG)** to orchestrate service startup, ensuring:
- Services start in the correct order
- Dependencies are satisfied before dependents start
- Failures cascade appropriately to dependent services

## Dependency Basics

### Simple Dependencies

The simplest form waits for a service to start:

```yaml
services:
  database:
    execute:
      type: docker
      image: postgres:15

  api:
    execute:
      type: pixi
      task: start-api
    dependencies:
      - database  # Wait for database to start
```

### Multiple Dependencies

Services can depend on multiple others:

```yaml
services:
  lidar:
    execute:
      type: ros2
      package: ldlidar_ros2
      launch_file: ldlidar.launch.py

  camera:
    execute:
      type: ros2
      package: realsense2_camera
      launch_file: rs_launch.py

  perception:
    execute:
      type: pixi
      task: run-perception
    dependencies:
      - lidar
      - camera  # Wait for both sensors
```

## Dependency Conditions

Control when a dependency is considered satisfied:

### `started` (Default)

Dependency is satisfied when the service process has started:

```yaml
dependencies:
  - service-name  # Shorthand for "started"
  - other-service: started  # Explicit
```

**Use when:**
- Service doesn't need to be fully initialized
- You just need the process running
- Fast startup is more important than readiness

### `healthy`

Dependency is satisfied when the service is healthy (requires health check):

```yaml
dependencies:
  - service-name: healthy
```

**Use when:**
- Dependent needs service to be fully operational
- Service exposes a health endpoint or port
- Correctness is more important than speed

**Requirements:**
- Dependency must have a `health_check` configured
- Health check must pass before dependent starts

### Mixed Conditions

Combine different conditions for different dependencies:

```yaml
services:
  database:
    execute:
      type: docker
      image: postgres:15
    health_check:
      type: tcp
      port: 5432

  cache:
    execute:
      type: docker
      image: redis:7
    health_check:
      type: tcp
      port: 6379

  worker:
    execute:
      type: shell
      command: python worker.py

  api:
    execute:
      type: pixi
      task: start-api
    dependencies:
      - database: healthy  # Must be ready for queries
      - cache: healthy     # Must be ready for caching
      - worker: started    # Just needs to be running
```

## Startup Order

### Linear Chain

Services start sequentially:

```yaml
services:
  step1:
    execute:
      type: shell
      command: ./init.sh

  step2:
    execute:
      type: pixi
      task: setup
    dependencies:
      - step1

  step3:
    execute:
      type: docker
      image: app:latest
    dependencies:
      - step2
```

**Startup:** step1 → step2 → step3

### Parallel Branches

Independent services start in parallel:

```yaml
services:
  # These start in parallel
  sensor-a:
    execute:
      type: ros2
      package: sensor_a
      launch_file: sensor.launch.py

  sensor-b:
    execute:
      type: ros2
      package: sensor_b
      launch_file: sensor.launch.py

  # This waits for both
  fusion:
    execute:
      type: pixi
      task: sensor-fusion
    dependencies:
      - sensor-a: healthy
      - sensor-b: healthy
```

**Startup:** sensor-a and sensor-b start together → fusion starts when both are healthy

### Diamond Pattern

Multiple paths converge:

```yaml
services:
  config:
    execute:
      type: shell
      command: ./load-config.sh

  service-a:
    execute:
      type: pixi
      task: start-a
    dependencies:
      - config

  service-b:
    execute:
      type: pixi
      task: start-b
    dependencies:
      - config

  aggregator:
    execute:
      type: docker
      image: aggregator:latest
    dependencies:
      - service-a: healthy
      - service-b: healthy
```

**Startup:** config → (service-a and service-b in parallel) → aggregator

### Layered Architecture

Build complex dependency graphs:

```yaml
services:
  # Layer 1: Hardware
  lidar:
    execute:
      type: ros2
      package: ldlidar_ros2
      launch_file: ldlidar.launch.py
    health_check:
      type: tcp
      port: 4048

  camera:
    execute:
      type: ros2
      package: realsense2_camera
      launch_file: rs_launch.py
    health_check:
      type: tcp
      port: 8554

  # Layer 2: Perception
  slam:
    execute:
      type: pixi
      task: run-slam
    dependencies:
      - lidar: healthy
      - camera: healthy
    health_check:
      type: heartbeat
      timeout: 5s

  object-detection:
    execute:
      type: docker
      image: detection:latest
    dependencies:
      - camera: healthy
    health_check:
      type: http
      port: 8080

  # Layer 3: Planning
  path-planner:
    execute:
      type: ros2
      package: nav2_bringup
      launch_file: navigation_launch.py
    dependencies:
      - slam: healthy
      - object-detection: healthy
    health_check:
      type: tcp
      port: 9090

  # Layer 4: Control
  controller:
    execute:
      type: pixi
      task: run-controller
    dependencies:
      - path-planner: healthy
    critical: true
```

## Failure Handling

### Cascading Failures

When a service fails, Krill automatically stops all dependent services:

```yaml
services:
  database:
    execute:
      type: docker
      image: postgres:15

  api:
    dependencies:
      - database
    # If database fails, api is automatically stopped
```

**Behavior:**
1. Database fails
2. Krill detects failure
3. API is stopped (cascade)
4. System settles into a safe state

### Critical Services

Mark services as critical to trigger emergency stop:

```yaml
services:
  safety-monitor:
    execute:
      type: pixi
      task: safety-check
    critical: true  # Failure stops ALL services
    health_check:
      type: heartbeat
      timeout: 1s

  motor-controller:
    dependencies:
      - safety-monitor: healthy
```

**Behavior:**
1. Safety-monitor fails
2. Krill triggers emergency stop
3. ALL services are stopped immediately
4. System enters safe state

### Restart Policies

Control how failures affect the dependency graph:

```yaml
services:
  flaky-sensor:
    execute:
      type: shell
      command: ./sensor-reader
    policy:
      restart: on-failure
      max_restarts: 3
      restart_delay: 2s
    health_check:
      type: tcp
      port: 5000

  processor:
    dependencies:
      - flaky-sensor: healthy
    # Waits for sensor to restart and become healthy
```

## Common Patterns

### Database-Backed Application

```yaml
services:
  postgres:
    execute:
      type: docker
      image: postgres:15
      volumes:
        - "./data:/var/lib/postgresql/data"
    health_check:
      type: tcp
      port: 5432
    policy:
      restart: on-failure

  migrations:
    execute:
      type: shell
      command: alembic upgrade head
    dependencies:
      - postgres: healthy
    # Runs once, exits when done

  backend:
    execute:
      type: pixi
      task: start-backend
    dependencies:
      - migrations  # Wait for migrations to complete
    health_check:
      type: http
      port: 8000
      path: /health
    policy:
      restart: always
```

### ROS2 Robot Stack

```yaml
services:
  # Hardware drivers
  motors:
    execute:
      type: ros2
      package: motor_driver
      launch_file: motors.launch.py
    health_check:
      type: tcp
      port: 7000

  sensors:
    execute:
      type: ros2
      package: sensor_suite
      launch_file: sensors.launch.py
    health_check:
      type: tcp
      port: 7001

  # Middle layer
  localization:
    execute:
      type: ros2
      package: robot_localization
      launch_file: ekf.launch.py
    dependencies:
      - motors: healthy
      - sensors: healthy

  # High level
  navigation:
    execute:
      type: ros2
      package: nav2_bringup
      launch_file: navigation_launch.py
    dependencies:
      - localization: started
    critical: true
```

### Microservices with Monitoring

```yaml
services:
  # Infrastructure
  prometheus:
    execute:
      type: docker
      image: prom/prometheus:latest
      ports:
        - "9090:9090"
    health_check:
      type: http
      port: 9090

  # Services
  auth-service:
    execute:
      type: docker
      image: auth:v1
    health_check:
      type: http
      port: 8001
      path: /health

  user-service:
    execute:
      type: docker
      image: users:v1
    dependencies:
      - auth-service: healthy
    health_check:
      type: http
      port: 8002

  api-gateway:
    execute:
      type: docker
      image: gateway:v1
      ports:
        - "80:80"
    dependencies:
      - auth-service: healthy
      - user-service: healthy
    health_check:
      type: http
      port: 80

  # Monitoring depends on all services starting
  grafana:
    execute:
      type: docker
      image: grafana/grafana:latest
      ports:
        - "3000:3000"
    dependencies:
      - prometheus: started
      - api-gateway: started
```

### Development Environment

```yaml
services:
  # Start database first
  dev-db:
    execute:
      type: docker
      image: postgres:15
      ports:
        - "5432:5432"
    health_check:
      type: tcp
      port: 5432

  # Run migrations
  dev-migrate:
    execute:
      type: shell
      command: npm run migrate
    dependencies:
      - dev-db: healthy

  # Start backend with hot reload
  dev-backend:
    execute:
      type: shell
      command: npm run dev
      working_dir: ./backend
    dependencies:
      - dev-migrate
    health_check:
      type: http
      port: 3001
    policy:
      restart: on-failure

  # Start frontend with hot reload
  dev-frontend:
    execute:
      type: shell
      command: npm run dev
      working_dir: ./frontend
    dependencies:
      - dev-backend: started
    health_check:
      type: http
      port: 3000
```

## Best Practices

### 1. Use Health Checks for Readiness

Always use `healthy` dependencies when the dependent truly needs the service ready:

```yaml
# ❌ Bad: API starts before DB is ready
api:
  dependencies:
    - database  # Just "started", might not be ready

# ✅ Good: API waits for DB to be ready
api:
  dependencies:
    - database: healthy
```

### 2. Minimize Dependency Chains

Shorter chains start faster and are easier to debug:

```yaml
# ❌ Bad: Long sequential chain
a: {}
b:
  dependencies: [a]
c:
  dependencies: [b]
d:
  dependencies: [c]

# ✅ Good: Parallel where possible
a: {}
b: {}
c: {}
d:
  dependencies: [a, b, c]
```

### 3. Use Critical Flag Sparingly

Reserve `critical` for truly safety-critical services:

```yaml
# ✅ Good: Critical for safety
emergency-stop:
  critical: true

# ❌ Bad: Dashboard isn't safety-critical
dashboard:
  critical: true  # Don't stop everything if dashboard fails
```

### 4. Layer Your Architecture

Group services into logical layers:

```yaml
# Layer 1: Infrastructure
# Layer 2: Data/Storage
# Layer 3: Business Logic
# Layer 4: API/Interface
```

### 5. Handle Circular Dependencies

Krill rejects circular dependencies. If you encounter this:

```yaml
# ❌ This will fail
service-a:
  dependencies: [service-b]
service-b:
  dependencies: [service-a]
```

**Solutions:**
- Redesign to remove circular dependency
- Split into smaller services
- Use message queues for loose coupling

### 6. Test Failure Scenarios

Verify your dependency graph handles failures correctly:

```bash
# Start system
krill up recipe.yaml

# Kill a service and observe cascades
krill service stop service-name

# Check dependent services stopped correctly
```

## Troubleshooting

### Services Start Out of Order

**Check:**
- Dependencies are correctly specified
- Health checks are configured for `healthy` dependencies
- No typos in service names

### Circular Dependency Error

**Solution:**
- Review your dependency graph
- Look for cycles (A → B → C → A)
- Redesign to break the cycle

### Service Waits Forever

**Possible causes:**
1. Dependency never becomes healthy
2. Dependency health check is misconfigured
3. Dependency service is failing

**Debug:**
```bash
# View service status
krill tui

# Check logs
krill logs dependency-name
```

### Cascading Failures Too Aggressive

**Solution:**
- Review critical flags
- Consider using restart policies
- May need to restructure dependencies

## See Also

- [Configuration Guide](configuration.md) - Full configuration reference
- [Health Checks Guide](health-checks.md) - Health check patterns
- [Best Practices](best-practices.md) - Production tips

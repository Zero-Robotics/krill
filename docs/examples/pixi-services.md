# Pixi Services

Three Python services orchestrated as a DAG, demonstrating health checks, restart policies, and the Krill Python SDK.

<!-- Video placeholder -->

## Architecture

```
data-processor          (no dependencies, always-restart)
       │
       ▼ healthy
data-analyzer           (restart on-failure, max 3)
       │
       ▼ healthy
decision-controller     (critical, restart on-failure, max 3)
```

## Recipe

```yaml title="examples/krill-pixi.yaml"
version: "1"
name: krill-pixi

env:
  ROBOT_ID: "pixi-001"
  LOG_LEVEL: "info"

services:
  data-processor:
    execute:
      type: pixi
      task: run-processor
      environment: default
      working_dir: ./services/data-processor
    dependencies: []
    health_check:
      type: heartbeat
      timeout: 10s
    policy:
      restart: always
      restart_delay: 5s

  data-analyzer:
    execute:
      type: pixi
      task: run-analyzer
      environment: default
      working_dir: ./services/data-analyzer
    dependencies:
      - data-processor: healthy
    health_check:
      type: heartbeat
      timeout: 10s
    policy:
      restart: on-failure
      max_restarts: 3
      restart_delay: 5s

  decision-controller:
    execute:
      type: pixi
      task: run-controller
      environment: default
      working_dir: ./services/decision-controller
    dependencies:
      - data-processor: healthy
      - data-analyzer: healthy
    critical: true
    health_check:
      type: heartbeat
      timeout: 15s
    policy:
      restart: on-failure
      max_restarts: 3
      restart_delay: 5s
```

## Running

```bash
# Install pixi environments (first time)
cd examples/services
for d in data-processor data-analyzer decision-controller; do
  (cd "$d" && pixi install)
done

# Start
cd ../..
krill up examples/krill-pixi.yaml
```

## What to Expect

1. **data-processor** starts first and begins sending heartbeats.
2. Once healthy, **data-analyzer** starts. It occasionally reports degraded status.
3. Once both are healthy, **decision-controller** starts.

## Try It

| Scenario | Command | Expected |
|----------|---------|----------|
| Kill processor | `kill <pid>` | Restarts immediately, dependents keep running |
| Kill analyzer | `kill <pid>` | Restarts up to 3 times, controller stops |
| Kill controller 3+ times | `kill <pid>` (repeat) | Emergency stop — all services shut down |

## Key Concepts

- **`type: pixi`** — runs a pixi task inside a managed environment
- **`dependencies: [svc: healthy]`** — waits for heartbeat confirmation before starting
- **`critical: true`** — failure of this service triggers a full emergency stop

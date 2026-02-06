# Krill Example Services

This directory contains a complete example of a robotics service orchestration using Krill.

## Architecture

```
┌─────────────────────┐
│  data-processor     │  No dependencies, always-restart
│  (Pixi Python)      │  Processes sensor data
└──────────┬──────────┘
           │ depends on (healthy)
           ▼
┌─────────────────────┐
│  data-analyzer      │  Depends on data-processor
│  (Pixi Python)      │  Analyzes processed data
└──────────┬──────────┘  Max 3 restart attempts
           │
           │ depends on (healthy)
           ▼
           │  ┌──────────────────────────┐
           └─▶│  decision-controller     │  Depends on both
              │  (Pixi Python)           │  Makes control decisions
              └──────────────────────────┘  Critical service, max 3 attempts
```

## Services

### 1. Data Processor
- **Dependencies:** None (starts first)
- **Policy:** Always restart (unlimited attempts)
- **Health:** Heartbeat every 1s
- **Function:** Processes raw sensor data

### 2. Data Analyzer
- **Dependencies:** data-processor (must be healthy)
- **Policy:** Restart on failure (max 3 attempts)
- **Health:** Heartbeat every 1s, reports degraded when anomalies detected
- **Function:** Analyzes processed data, detects anomalies

### 3. Decision Controller
- **Dependencies:** data-processor AND data-analyzer (both must be healthy)
- **Policy:** Restart on failure (max 3 attempts)
- **Critical:** Yes (failure triggers emergency stop of all services)
- **Health:** Heartbeat every 1s with decision metadata
- **Function:** Makes control decisions based on analyzed data

## Prerequisites

1. **Pixi installed:**
   ```bash
   curl -fsSL https://pixi.sh/install.sh | bash
   ```

2. **Krill daemon built:**
   ```bash
   cd /Users/tommaso/dev/zero/krill
   cargo build --release
   ```

3. **Python SDK available:**
   The services assume the Python SDK is at `../../../sdk/krill-python/`

## Running the Example

### 1. Initialize Pixi environments (first time only)

```bash
# From the examples/services directory
cd data-processor && pixi install && cd ..
cd data-analyzer && pixi install && cd ..
cd decision-controller && pixi install && cd ..
```

### 2. Start the Krill daemon

```bash
# From the krill root directory
cargo run --bin krill-daemon -- --config examples/krill-example.yaml
```

### 3. Monitor with TUI (in another terminal)

```bash
cargo run --bin krill ps
```

## Expected Behavior

When running, you should observe:

1. **Startup Order:**
   - `data-processor` starts first (no dependencies)
   - `data-analyzer` starts when `data-processor` is healthy
   - `decision-controller` starts when both are healthy

2. **Health Reporting:**
   - All services send heartbeats every second
   - `data-analyzer` occasionally reports degraded status (every 10th iteration)
   - All services show green/healthy in TUI

3. **Restart Behavior:**
   - If `data-processor` crashes, it restarts immediately (always policy)
   - If `data-analyzer` or `decision-controller` crash, they restart up to 3 times
   - If `decision-controller` fails after 3 attempts, **emergency stop** triggers (all services stop)

4. **Dependency Cascading:**
   - If `data-processor` stops, `data-analyzer` and `decision-controller` also stop
   - If `data-analyzer` stops, only `decision-controller` stops

## Testing Scenarios

### Test 1: Kill data-processor
```bash
# Find PID from TUI or ps
kill <data-processor-pid>
# Observe: Immediate restart, dependents continue running
```

### Test 2: Kill data-analyzer
```bash
kill <data-analyzer-pid>
# Observe: Restarts up to 3 times, decision-controller stops (dependency)
```

### Test 3: Kill decision-controller (critical service)
```bash
kill <decision-controller-pid>
# Observe: Restarts up to 3 times, then triggers emergency stop of ALL services
```

## Customization

Modify `krill-example.yaml` to experiment with:
- Different restart policies
- Different health check timeouts
- Adding more services
- Changing the dependency graph
- Adjusting restart delays

## Troubleshooting

**Services not starting:**
- Check pixi environments are installed: `pixi list` in each service directory
- Verify Python SDK path in each Python script

**Health checks failing:**
- Ensure Krill daemon is running: `ls -la /tmp/krill.sock`
- Check service logs: `cargo run --bin krill logs -- <service-name>`

**Dependencies not working:**
- Verify dependency names match service names in YAML
- Check TUI to see service status (must be "Healthy" not just "Running")

# Health Checks Guide

Learn how to effectively monitor your services with Krill's health check system.

## Overview

Health checks allow Krill to:
- Monitor service health continuously
- Determine when dependencies are satisfied (`healthy` condition)
- Trigger restarts on health failures (with `on-failure` policy)
- Report accurate service status in the TUI

## Health Check Types

### Heartbeat

**Best for:** Custom services where you control the code

Services actively report they're alive using Krill SDKs.

```yaml
health_check:
  type: heartbeat
  timeout: 5s
```

**How it works:**
1. Service must send heartbeats within the timeout period
2. If timeout expires without a heartbeat, service is marked unhealthy
3. Heartbeats can be sent from Rust, Python, or C++ using Krill SDKs

**Rust Example:**

```rust
use krill_sdk_rust::KrillClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = KrillClient::new("my-service").await?;
    
    loop {
        // Do work...
        process_data().await?;
        
        // Report health
        client.heartbeat().await?;
        
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }
}
```

**Python Example:**

```python
from krill import KrillClient
import time

with KrillClient("my-service") as client:
    while True:
        # Do work...
        process_data()
        
        # Report health
        client.heartbeat()
        
        time.sleep(2)
```

**Python Async Example:**

```python
import asyncio
from krill import AsyncKrillClient

async def main():
    client = await AsyncKrillClient.connect("my-service")
    
    while True:
        # Do work...
        await process_data()
        
        # Report health
        await client.heartbeat()
        
        await asyncio.sleep(2)

asyncio.run(main())
```

**C++ Example:**

```cpp
#include "krill.hpp"
#include <thread>
#include <chrono>

int main() {
    try {
        krill::Client client("my-service");
        
        while (true) {
            // Do work...
            process_data();
            
            // Report health
            client.heartbeat();
            
            std::this_thread::sleep_for(std::chrono::seconds(2));
        }
    } catch (const krill::KrillError& e) {
        std::cerr << "Error: " << e.what() << std::endl;
        return 1;
    }
}
```

**Best Practices:**
- Set timeout 2-3x your heartbeat interval for safety margin
- Send heartbeats from your main processing loop
- Don't send heartbeats if your service is degraded

### TCP

**Best for:** Services that expose a TCP port

Checks if a TCP port accepts connections.

```yaml
health_check:
  type: tcp
  port: 8080
  timeout: 2s
```

**How it works:**
1. Krill attempts to establish a TCP connection to `localhost:port`
2. If connection succeeds, service is healthy
3. If connection fails or times out, service is unhealthy

**Use Cases:**
- Databases (PostgreSQL, Redis)
- Message brokers (RabbitMQ, Kafka)
- Any service listening on a TCP port

**Example:**

```yaml
services:
  redis:
    execute:
      type: shell
      command: redis-server
    health_check:
      type: tcp
      port: 6379
      timeout: 1s
```

### HTTP

**Best for:** Web services and REST APIs

Performs HTTP GET requests to a health endpoint.

```yaml
health_check:
  type: http
  port: 8080
  path: /health
  expected_status: 200
```

**How it works:**
1. Krill sends `GET http://localhost:port/path`
2. If response status matches `expected_status`, service is healthy
3. Otherwise, service is unhealthy

**Implementing a Health Endpoint:**

```python
# FastAPI example
from fastapi import FastAPI

app = FastAPI()

@app.get("/health")
async def health_check():
    # Add your health logic here
    if database.is_connected() and cache.is_available():
        return {"status": "healthy"}
    else:
        return {"status": "unhealthy"}, 503
```

**Example:**

```yaml
services:
  api-server:
    execute:
      type: pixi
      task: start-api
    health_check:
      type: http
      port: 8080
      path: /api/health
      expected_status: 200
```

### Script

**Best for:** Custom health logic that doesn't fit other types

Runs a shell command to determine health.

```yaml
health_check:
  type: script
  command: curl -f http://localhost:8080/health
  timeout: 3s
```

**How it works:**
1. Krill executes the command
2. Exit code 0 = healthy
3. Non-zero exit code = unhealthy

**Use Cases:**
- Complex health checks requiring multiple conditions
- Checking file existence or content
- Custom validation logic

**Examples:**

```yaml
# Check if service responds AND database is accessible
health_check:
  type: script
  command: curl -f http://localhost:8080/health && pg_isready -h localhost
  timeout: 5s
```

```yaml
# Check if a file exists and was modified recently
health_check:
  type: script
  command: test -f /var/run/service.pid && find /var/run/service.pid -mmin -1
  timeout: 1s
```

## Choosing the Right Health Check

| Service Type | Recommended Check | Reason |
|--------------|------------------|---------|
| Custom application (you control code) | **Heartbeat** | Most accurate, reports actual internal state |
| ROS2 nodes | **TCP** or **Heartbeat** | ROS2 nodes often expose ports; heartbeat for custom nodes |
| Web APIs | **HTTP** | Native support for health endpoints |
| Databases | **TCP** | Simple connection test |
| Docker containers | **TCP** or **HTTP** | Depends on what container exposes |
| Shell scripts | **Script** | Custom validation logic |

## Health Check Strategies

### Fast Startup Detection

Use TCP/HTTP checks for services that expose ports immediately:

```yaml
services:
  nginx:
    execute:
      type: docker
      image: nginx:latest
      ports:
        - "80:80"
    health_check:
      type: http
      port: 80
      path: /
      expected_status: 200
```

### Accurate State Monitoring

Use heartbeat for services where you need to know internal state:

```yaml
services:
  data-processor:
    execute:
      type: pixi
      task: run-processor
    health_check:
      type: heartbeat
      timeout: 10s
```

In your code:

```python
with KrillClient("data-processor") as client:
    while True:
        try:
            data = queue.get(timeout=5)
            process(data)
            client.heartbeat()  # Only heartbeat when actually processing
        except QueueEmpty:
            # Don't heartbeat when idle - this will mark as unhealthy
            pass
```

### Multi-Layer Checks

Combine different checks for different services:

```yaml
services:
  # Hardware: TCP check for quick startup detection
  lidar:
    execute:
      type: ros2
      package: ldlidar_ros2
      launch_file: ldlidar.launch.py
    health_check:
      type: tcp
      port: 4048

  # Processing: Heartbeat for accurate state
  slam:
    execute:
      type: pixi
      task: run-slam
    dependencies:
      - lidar: healthy
    health_check:
      type: heartbeat
      timeout: 5s

  # API: HTTP for standard health endpoint
  api:
    execute:
      type: docker
      image: api-server:latest
    dependencies:
      - slam: healthy
    health_check:
      type: http
      port: 8080
      path: /health
```

## Health Check Timing

### Startup Phase

- Services start as `starting`
- First successful health check → `healthy`
- Failed health checks don't trigger restarts during initial startup

### Running Phase

- Continuous health monitoring
- Health failures can trigger `on-failure` restarts
- `healthy` dependencies wait for health checks to pass

### Shutdown Phase

- Health checks stop when service is stopping
- No false negatives during graceful shutdown

## Common Patterns

### Critical Services with Monitoring

```yaml
services:
  motion-controller:
    execute:
      type: ros2
      package: motion_control
      launch_file: controller.launch.py
    critical: true  # Emergency stop if this fails
    health_check:
      type: heartbeat
      timeout: 2s  # Short timeout for safety-critical
    policy:
      restart: on-failure
      max_restarts: 1  # Don't retry infinitely
```

### Dependent Startup Chain

```yaml
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
      task: start-backend
    dependencies:
      - database: healthy  # Wait for DB to be ready
    health_check:
      type: http
      port: 8000
      path: /health

  frontend:
    execute:
      type: docker
      image: frontend:latest
    dependencies:
      - backend: healthy  # Wait for backend to be ready
    health_check:
      type: http
      port: 3000
```

### Resilient Services

```yaml
services:
  sensor-reader:
    execute:
      type: pixi
      task: read-sensors
    health_check:
      type: heartbeat
      timeout: 10s  # Allow some processing time
    policy:
      restart: always  # Always restart on failure
      max_restarts: 0  # Unlimited retries
      restart_delay: 5s  # Wait before retry
```

## Troubleshooting

### Service Never Becomes Healthy

**TCP/HTTP checks:**
- Verify the service is actually listening on the specified port
- Check firewall rules
- Ensure `localhost` resolves correctly

**Heartbeat checks:**
- Verify SDK is correctly initialized
- Check that heartbeats are being sent
- Review timeout duration vs heartbeat frequency

**Script checks:**
- Run the command manually to verify it works
- Check command output and exit codes
- Verify timeouts are sufficient

### Intermittent Health Failures

**Symptoms:** Service flaps between healthy and unhealthy

**Solutions:**
- Increase health check timeout
- Reduce heartbeat frequency vs timeout ratio
- Check for resource contention (CPU, memory)
- Review service logs for errors

### Dependencies Never Satisfied

**Symptoms:** Service waits forever for dependency

**Solutions:**
- Check that dependency has a health check configured
- Verify dependency service is starting successfully
- Review dependency health check configuration
- Check dependency service logs

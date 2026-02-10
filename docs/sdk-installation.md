# SDK Installation and Usage Guide

Learn how to install and use Krill SDKs in your services for health monitoring via heartbeats.

## Table of Contents

- [Overview](#overview)
- [Python SDK](#python-sdk)
- [Rust SDK](#rust-sdk)
- [C++ SDK](#c-sdk)
- [When to Use SDKs](#when-to-use-sdks)
- [Examples](#examples)

## Overview

Krill SDKs allow your services to report their health status via **heartbeat health checks**. This is the most accurate health monitoring method because your code explicitly signals when it's healthy.

**Key Concepts:**
- Services send periodic heartbeats to the Krill daemon
- If heartbeats stop, the service is marked unhealthy
- Works over Unix domain sockets (IPC)
- Zero-copy, lightweight communication

## Python SDK

### Installation

**Option 1: Install from source (recommended)**

```bash
cd krill/sdk/krill-python
pip install .
```

**Option 2: Install in development mode**

```bash
cd krill/sdk/krill-python
pip install -e .
```

**Option 3: Copy to your project**

```bash
# Just copy the single file
cp krill/sdk/krill-python/krill.py your-project/
```

### Requirements

- Python 3.7 or later
- No external dependencies (uses only stdlib)
- Unix-like system (Linux, macOS)

### Basic Usage

```python
from krill import KrillClient
import time

# Context manager (recommended)
with KrillClient("my-service") as client:
    while True:
        # Do your work
        process_data()
        
        # Send heartbeat
        client.heartbeat()
        
        time.sleep(1)
```

### Async Usage

```python
import asyncio
from krill import AsyncKrillClient

async def main():
    async with await AsyncKrillClient.connect("my-service") as client:
        while True:
            # Do async work
            await process_data()
            
            # Send heartbeat
            await client.heartbeat()
            
            await asyncio.sleep(1)

asyncio.run(main())
```

### Configuration in Recipe

```yaml
services:
  my-service:
    execute:
      type: pixi
      task: run-service
    health_check:
      type: heartbeat
      timeout: 5s  # Must send heartbeat every 5s
```

### Complete Example

```python
#!/usr/bin/env python3
"""Example service with Krill heartbeat monitoring."""

from krill import KrillClient, ConnectionError, SendError
import time
import sys

def process_frame():
    """Simulate some work."""
    time.sleep(0.1)
    return {"frame": 42, "fps": 30}

def main():
    try:
        # Connect to Krill daemon
        with KrillClient("vision-processor") as client:
            print("Connected to Krill daemon")
            
            frame_count = 0
            while True:
                try:
                    # Do work
                    result = process_frame()
                    frame_count += 1
                    
                    # Send heartbeat every 10 frames
                    if frame_count % 10 == 0:
                        client.heartbeat_with_metadata({
                            "frames_processed": str(frame_count),
                            "fps": str(result["fps"])
                        })
                        print(f"Heartbeat sent (frame {frame_count})")
                    
                except KeyboardInterrupt:
                    print("\nShutting down...")
                    break
                    
    except ConnectionError as e:
        print(f"Cannot connect to Krill daemon: {e}", file=sys.stderr)
        print("Make sure Krill daemon is running", file=sys.stderr)
        return 1
        
    except SendError as e:
        print(f"Failed to send heartbeat: {e}", file=sys.stderr)
        return 1
    
    return 0

if __name__ == "__main__":
    sys.exit(main())
```

### API Reference

**KrillClient (Synchronous)**

```python
# Constructor
client = KrillClient(service_name: str, socket_path: str = "/tmp/krill.sock")

# Methods
client.heartbeat()  # Send heartbeat
client.heartbeat_with_metadata(metadata: dict[str, str])  # With metadata
client.report_degraded(reason: str)  # Report degraded status
client.report_healthy()  # Report healthy status
client.close()  # Close connection

# Context manager
with KrillClient("service-name") as client:
    client.heartbeat()
```

**AsyncKrillClient (Asynchronous)**

```python
# Constructor (async)
client = await AsyncKrillClient.connect(service_name: str, socket_path: str = "/tmp/krill.sock")

# Methods (all async)
await client.heartbeat()
await client.heartbeat_with_metadata(metadata: dict[str, str])
await client.report_degraded(reason: str)
await client.report_healthy()
await client.close()

# Context manager (async)
async with await AsyncKrillClient.connect("service-name") as client:
    await client.heartbeat()
```

## Rust SDK

### Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
krill-sdk-rust = { path = "../krill/crates/krill-sdk-rust" }
tokio = { version = "1", features = ["full"] }
```

Or if published to crates.io:

```toml
[dependencies]
krill-sdk-rust = "0.1"
tokio = { version = "1", features = ["full"] }
```

### Requirements

- Rust 1.70 or later
- Tokio runtime (for async)
- Unix-like system (Linux, macOS)

### Basic Usage

```rust
use krill_sdk_rust::KrillClient;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Connect to Krill daemon
    let client = KrillClient::new("my-service").await?;
    
    loop {
        // Do your work
        process_data().await?;
        
        // Send heartbeat
        client.heartbeat().await?;
        
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
```

### Configuration in Recipe

```yaml
services:
  my-service:
    execute:
      type: shell
      command: ./target/release/my-service
    health_check:
      type: heartbeat
      timeout: 5s
```

### Complete Example

```rust
use krill_sdk_rust::{KrillClient, KrillError};
use std::time::Duration;
use tokio::time;

async fn process_data() -> Result<(), Box<dyn std::error::Error>> {
    // Simulate work
    time::sleep(Duration::from_millis(100)).await;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting vision processor...");
    
    // Connect to Krill daemon
    let client = match KrillClient::new("vision-processor").await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Cannot connect to Krill daemon: {}", e);
            eprintln!("Make sure Krill daemon is running");
            return Err(e.into());
        }
    };
    
    println!("Connected to Krill daemon");
    
    let mut frame_count = 0u64;
    
    loop {
        // Process frame
        if let Err(e) = process_data().await {
            eprintln!("Error processing data: {}", e);
            continue;
        }
        
        frame_count += 1;
        
        // Send heartbeat every 10 frames
        if frame_count % 10 == 0 {
            let mut metadata = std::collections::HashMap::new();
            metadata.insert("frames_processed".to_string(), frame_count.to_string());
            metadata.insert("fps".to_string(), "30".to_string());
            
            if let Err(e) = client.heartbeat_with_metadata(metadata).await {
                eprintln!("Failed to send heartbeat: {}", e);
                return Err(e.into());
            }
            
            println!("Heartbeat sent (frame {})", frame_count);
        }
    }
}
```

### API Reference

```rust
// Constructor (async)
let client = KrillClient::new(service_name: &str).await?;
let client = KrillClient::with_socket_path(service_name: &str, socket_path: &str).await?;

// Methods (all async)
client.heartbeat().await?;
client.heartbeat_with_metadata(metadata: HashMap<String, String>).await?;
client.report_degraded(reason: &str).await?;
client.report_healthy().await?;
```

## C++ SDK

### Installation

The C++ SDK is **header-only**. Just include the header:

```cpp
#include "krill/sdk/krill-cpp/krill.hpp"
```

### Requirements

- C++11 or later
- No external dependencies
- Unix-like system (Linux, macOS)

### Basic Usage

```cpp
#include "krill.hpp"
#include <thread>
#include <chrono>
#include <iostream>

int main() {
    try {
        // Connect to Krill daemon
        krill::Client client("my-service");
        
        while (true) {
            // Do your work
            process_data();
            
            // Send heartbeat
            client.heartbeat();
            
            std::this_thread::sleep_for(std::chrono::seconds(1));
        }
        
    } catch (const krill::KrillError& e) {
        std::cerr << "Error: " << e.what() << std::endl;
        return 1;
    }
    
    return 0;
}
```

### Configuration in Recipe

```yaml
services:
  my-service:
    execute:
      type: shell
      command: ./build/my-service
    health_check:
      type: heartbeat
      timeout: 5s
```

### Complete Example

```cpp
#include "krill.hpp"
#include <iostream>
#include <thread>
#include <chrono>
#include <map>

void process_frame() {
    // Simulate work
    std::this_thread::sleep_for(std::chrono::milliseconds(100));
}

int main() {
    std::cout << "Starting vision processor..." << std::endl;
    
    try {
        // Connect to Krill daemon
        krill::Client client("vision-processor");
        std::cout << "Connected to Krill daemon" << std::endl;
        
        uint64_t frame_count = 0;
        
        while (true) {
            // Process frame
            process_frame();
            frame_count++;
            
            // Send heartbeat every 10 frames
            if (frame_count % 10 == 0) {
                std::map<std::string, std::string> metadata;
                metadata["frames_processed"] = std::to_string(frame_count);
                metadata["fps"] = "30";
                
                client.heartbeat_with_metadata(metadata);
                std::cout << "Heartbeat sent (frame " << frame_count << ")" << std::endl;
            }
        }
        
    } catch (const krill::KrillError& e) {
        std::cerr << "Error: " << e.what() << std::endl;
        std::cerr << "Make sure Krill daemon is running" << std::endl;
        return 1;
    }
    
    return 0;
}
```

### Compilation

```bash
# Simple compilation
g++ -std=c++11 -I/path/to/krill/sdk -o my-service my-service.cpp

# With optimizations
g++ -std=c++11 -O3 -I/path/to/krill/sdk -o my-service my-service.cpp

# With CMake
# CMakeLists.txt:
cmake_minimum_required(VERSION 3.10)
project(MyService)

set(CMAKE_CXX_STANDARD 11)

include_directories(/path/to/krill/sdk)

add_executable(my-service my-service.cpp)
```

### API Reference

```cpp
// Constructor
krill::Client client(const std::string& service_name);
krill::Client client(const std::string& service_name, const std::string& socket_path);

// Methods
client.heartbeat();  // Send heartbeat
client.heartbeat_with_metadata(const std::map<std::string, std::string>& metadata);
client.report_degraded(const std::string& reason);
client.report_healthy();

// All methods may throw krill::KrillError
```

## When to Use SDKs

### ✅ Use SDK Heartbeats When:

- You control the service code
- You need accurate internal state monitoring
- Service has complex initialization
- You want to report degraded states
- Service does async/background work

### ❌ Don't Use SDK Heartbeats When:

- You can't modify the service code
- Service is a third-party binary
- Simple TCP/HTTP check is sufficient
- Service has no long-running process

**Alternatives:**
- **TCP health check** - For services exposing ports
- **HTTP health check** - For web services/APIs
- **Script health check** - For custom validation

## Examples

### ROS2 Node with Heartbeat

```python
#!/usr/bin/env python3
import rclpy
from rclpy.node import Node
from krill import KrillClient

class MyNode(Node):
    def __init__(self):
        super().__init__('my_node')
        
        # Initialize Krill client
        self.krill_client = KrillClient("my-ros-node")
        
        # Create timer for heartbeats (every 2 seconds)
        self.heartbeat_timer = self.create_timer(2.0, self.send_heartbeat)
        
        # Your ROS2 logic here
        self.create_subscription(...)
    
    def send_heartbeat(self):
        try:
            self.krill_client.heartbeat()
            self.get_logger().debug("Heartbeat sent")
        except Exception as e:
            self.get_logger().error(f"Failed to send heartbeat: {e}")

def main():
    rclpy.init()
    node = MyNode()
    
    try:
        rclpy.spin(node)
    finally:
        node.krill_client.close()
        node.destroy_node()
        rclpy.shutdown()

if __name__ == '__main__':
    main()
```

**Recipe:**

```yaml
services:
  my-ros-node:
    execute:
      type: shell
      command: python3 my_node.py
    health_check:
      type: heartbeat
      timeout: 5s
```

### Data Processing Pipeline

```python
from krill import KrillClient
import time
import queue

def main():
    with KrillClient("data-processor") as client:
        work_queue = queue.Queue()
        
        while True:
            try:
                # Get work with timeout
                item = work_queue.get(timeout=2.0)
                
                # Process item
                result = process(item)
                
                # Only heartbeat when actively processing
                client.heartbeat_with_metadata({
                    "queue_size": str(work_queue.qsize()),
                    "items_processed": str(result.count)
                })
                
            except queue.Empty:
                # No work available - don't heartbeat
                # This will cause service to be marked unhealthy
                # if no work arrives within the timeout
                pass
```

### Microservice with Health States

```python
from krill import KrillClient
from enum import Enum

class HealthState(Enum):
    HEALTHY = "healthy"
    DEGRADED = "degraded"
    UNHEALTHY = "unhealthy"

def main():
    with KrillClient("api-service") as client:
        state = HealthState.HEALTHY
        
        while True:
            # Check system resources
            cpu_usage = get_cpu_usage()
            memory_usage = get_memory_usage()
            
            # Determine health state
            if cpu_usage > 90 or memory_usage > 95:
                state = HealthState.DEGRADED
                client.report_degraded(
                    f"High resource usage: CPU={cpu_usage}% MEM={memory_usage}%"
                )
            elif cpu_usage < 70 and memory_usage < 80:
                if state == HealthState.DEGRADED:
                    client.report_healthy()
                    state = HealthState.HEALTHY
                client.heartbeat()
            
            time.sleep(1)
```

## Troubleshooting

### Connection Failed

**Error:** `Cannot connect to Krill daemon`

**Solutions:**
1. Verify Krill daemon is running: `ps aux | grep krill`
2. Check socket path is correct (default: `/tmp/krill.sock`)
3. Ensure service name matches recipe configuration
4. Check file permissions on socket

### Heartbeats Not Received

**Symptoms:** Service always shows as unhealthy

**Solutions:**
1. Verify heartbeat timeout in recipe is sufficient
2. Check service is actually calling `heartbeat()`
3. Review service logs for SDK errors
4. Ensure heartbeat frequency < timeout

### Service Marked Unhealthy During Startup

**Symptoms:** Service fails health check immediately

**Solutions:**
1. Increase health check timeout in recipe
2. Send first heartbeat as soon as possible
3. Consider using `started` dependency instead of `healthy`

# Krill C++ SDK

Header-only C++ client library for sending heartbeats to the Krill daemon.

## Requirements

- C++11 or later
- Unix-like system (Linux, macOS)
- Krill daemon running

## Installation

Simply include the `krill.hpp` header in your project:

```cpp
#include "krill.hpp"
```

## Usage

### Basic Heartbeat

```cpp
#include "krill.hpp"

int main() {
    try {
        krill::Client client("my-service");
        client.heartbeat();
    } catch (const krill::KrillError& e) {
        std::cerr << "Error: " << e.what() << std::endl;
        return 1;
    }
    return 0;
}
```

### Heartbeat with Metadata

```cpp
std::map<std::string, std::string> metadata;
metadata["fps"] = "29.5";
metadata["latency_ms"] = "15";

client.heartbeat_with_metadata(metadata);
```

### Report Degraded Status

```cpp
client.report_degraded("High latency detected");
```

### Custom Socket Path

```cpp
krill::Client client("my-service", "/var/run/krill.sock");
```

## Compilation

```bash
g++ -std=c++11 -o my_app my_app.cpp
```

## Thread Safety

The Client class is **not thread-safe**. Create separate Client instances for each thread, or protect access with a mutex.

## Error Handling

All methods may throw `krill::KrillError` on failure. Always wrap calls in try-catch blocks.

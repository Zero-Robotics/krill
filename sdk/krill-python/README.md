# Krill Python SDK

Python client library for sending heartbeats to the Krill daemon.

## Features

- **Zero dependencies** - Uses only the Python standard library
- **Sync and async** - Supports both synchronous and asyncio usage
- **Thread-safe** - Synchronous client is protected by locks
- **Type hints** - Full type annotations for IDE support
- **Simple API** - Clean interface matching Rust and C++ SDKs

## Requirements

- Python 3.7 or later
- Unix-like system (Linux, macOS)
- Krill daemon running

## Installation

Copy `krill.py` to your project or add it to your Python path:

```bash
# Copy to your project
cp krill.py /path/to/your/project/

# Or install to site-packages
python3 -m pip install --user /path/to/krill-python/
```

## Usage

### Synchronous API

```python
from krill import KrillClient

# Create client
client = KrillClient("my-service")

# Send heartbeat
client.heartbeat()

# With metadata
client.heartbeat_with_metadata({
    "fps": "29.5",
    "latency_ms": "15"
})

# Report degraded status
client.report_degraded("High latency detected")

# Report healthy status
client.report_healthy()

# Close connection
client.close()
```

### Context Manager (Recommended)

```python
from krill import KrillClient

with KrillClient("my-service") as client:
    client.heartbeat()
    # Connection automatically closed on exit
```

### Asynchronous API

```python
import asyncio
from krill import AsyncKrillClient

async def main():
    # Connect to daemon
    client = await AsyncKrillClient.connect("my-service")
    
    # Send heartbeat
    await client.heartbeat()
    
    # With metadata
    await client.heartbeat_with_metadata({
        "fps": "29.5",
        "latency_ms": "15"
    })
    
    # Report degraded status
    await client.report_degraded("High latency detected")
    
    # Report healthy status
    await client.report_healthy()
    
    # Close connection
    await client.close()

asyncio.run(main())
```

### Async Context Manager

```python
async def main():
    async with await AsyncKrillClient.connect("my-service") as client:
        await client.heartbeat()
        # Connection automatically closed on exit

asyncio.run(main())
```

### Custom Socket Path

```python
# Synchronous
client = KrillClient("my-service", socket_path="/var/run/krill.sock")

# Asynchronous
client = await AsyncKrillClient.connect("my-service", socket_path="/var/run/krill.sock")
```

## Complete Example

```python
#!/usr/bin/env python3
from krill import KrillClient
import time

def main():
    try:
        client = KrillClient("vision-pipeline")
        
        for i in range(10):
            # Do work...
            time.sleep(1)
            
            # Send heartbeat
            if i % 3 == 0:
                metadata = {"frame": str(i), "fps": "30"}
                client.heartbeat_with_metadata(metadata)
            else:
                client.heartbeat()
        
        client.close()
        
    except Exception as e:
        print(f"Error: {e}")
        return 1
    
    return 0

if __name__ == "__main__":
    exit(main())
```

See `example.py` for synchronous usage and `example_async.py` for async usage.

## Error Handling

The SDK raises the following exceptions:

- `KrillError` - Base exception class
- `ConnectionError` - Failed to connect to daemon
- `SendError` - Failed to send a message

```python
from krill import KrillClient, ConnectionError, SendError

try:
    client = KrillClient("my-service")
    client.heartbeat()
except ConnectionError as e:
    print(f"Cannot connect to daemon: {e}")
except SendError as e:
    print(f"Failed to send heartbeat: {e}")
```

## Thread Safety

- **`KrillClient`** (sync) - Thread-safe. Multiple threads can share one instance.
- **`AsyncKrillClient`** (async) - Not thread-safe. Use one instance per event loop.

## Testing

Run the test suite:

```bash
python3 tests/test_krill.py
```

Or with pytest:

```bash
pytest tests/
```

## Performance

The SDK has minimal overhead:
- No external dependencies (only stdlib)
- Single socket connection per client
- JSON serialization via standard library
- Async version uses asyncio for non-blocking I/O

## Comparison with Other SDKs

| Feature | Python (sync) | Python (async) | Rust | C++ |
|---------|---------------|----------------|------|-----|
| Zero dependencies | ✓ | ✓ | ✗ | ✓ |
| Thread-safe | ✓ | ✗ | ✓ | ✗ |
| Non-blocking | ✗ | ✓ | ✓ | ✗ |
| Type hints | ✓ | ✓ | ✓ | ✓ |

## License

Apache-2.0

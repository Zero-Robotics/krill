# Test Shell Configuration

This configuration demonstrates Krill's service orchestration with simple shell scripts.

## Services

1. **counter1** - Counts from 0, incrementing every 1 second
   - Base service with no dependencies
   - Restart policy: always

2. **counter2** - Counts from 100, incrementing every 2 seconds
   - Depends on: counter1
   - Restart policy: always

3. **fast_counter** - Counts from 0, incrementing every 0.5 seconds
   - No dependencies (runs independently)
   - Restart policy: always

4. **logger** - Prints timestamped heartbeat every 3 seconds
   - Depends on: counter1, counter2
   - Restart policy: always

5. **failing_service** - Starts, waits 5 seconds, then exits with error
   - Demonstrates restart policies
   - Restart policy: on-failure (max 3 restarts)

## Running

Start the daemon:
```bash
cd /Users/tommaso/Documents/dev/krill
./target/release/krill-daemon --config examples/test-shell.yaml
```

In another terminal, launch the TUI:
```bash
./target/release/krill-tui
```

## TUI Controls

- **↑/↓ or j/k**: Navigate services
- **Enter**: View service details
- **l**: View service logs
- **Esc**: Go back
- **q**: Quit

## What to Observe

- Services start in dependency order (counter1 → counter2 → logger)
- fast_counter and failing_service start independently
- failing_service will fail and restart up to 3 times
- All counters increment at different rates
- Logger shows dependency on both counters

## Stopping

Press **Ctrl+C** in the daemon terminal to gracefully shutdown all services.

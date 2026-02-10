# Shell Scripts

Simple shell scripts demonstrating dependency chains and failure handling.

<!-- Video placeholder -->

## Recipe

```yaml title="examples/krill-shell.yaml"
version: "1"
name: test-shell
log_dir: ~/.krill/logs

services:
  counter1:
    execute:
      type: shell
      command: bash counter1.sh
      working_dir: ./scripts/
    policy:
      restart: always

  counter2:
    execute:
      type: shell
      command: bash counter2.sh
      working_dir: ./scripts/
    dependencies:
      - counter1
    policy:
      restart: always

  fast_counter:
    execute:
      type: shell
      command: bash fast_counter.sh
      working_dir: ./scripts/
    policy:
      restart: always

  logger:
    execute:
      type: shell
      command: bash logger.sh
      working_dir: ./scripts/
    dependencies:
      - counter1
      - counter2
    policy:
      restart: always

  failing_service:
    execute:
      type: shell
      command: bash failing.sh
      working_dir: ./scripts/
    policy:
      restart: on-failure
      max_restarts: 3
      restart_delay: 2s
```

## Running

```bash
krill up examples/krill-shell.yaml
```

## What to Expect

- **counter1** and **fast_counter** start immediately (no dependencies).
- **counter2** starts after counter1 is up.
- **logger** starts after both counter1 and counter2.
- **failing_service** runs for 5 seconds, exits with code 1, then restarts up to 3 times before giving up.

## Key Concepts

- **`type: shell`** — runs any shell command.
- **`restart: on-failure` + `max_restarts`** — automatic recovery with a retry budget.
- Good starting point for testing Krill without any language-specific tooling.

# Getting Started

## Prerequisites

-   [Rust toolchain](https://rustup.rs/) installed
-   [Just](https://just.systems/) task runner installed

## Installation

```bash
git clone https://github.com/Zero-Robotics/krill.git
cd krill
just install
```

This builds and installs the `krill` binary to your Cargo bin directory.

## Your First Recipe

Create a file called `krill.yaml`:

```yaml
version: "1"
name: my-first-recipe

services:
  hello:
    execute:
      type: shell
      command: bash -c 'while true; do echo "Hello from Krill!"; sleep 2; done'
    policy:
      restart: always
```

## Start It

```bash
krill up krill.yaml
```

This starts the daemon and opens the TUI where you can monitor your services in real time.

## TUI Controls

| Key | Action |
|-----|--------|
| `↑`/`↓` | Navigate services |
| `Enter` | View service logs |
| `d` | Service detail view |
| `r` | Restart service |
| `s` | Stop service |
| `q` | Quit TUI |

## Stop Everything

Press `q` in the TUI, or from another terminal:

```bash
krill down
```

## Detached Mode

Start without the TUI:

```bash
krill up krill.yaml -d
```

Attach later with:

```bash
krill ps
```

## Next Steps

- Browse the [Examples](examples/index.md) to see real-world recipes
- Read the [Configuration Reference](configuration.md) for all options
- Check the [Quick Reference](quick-reference.md) for a cheat sheet

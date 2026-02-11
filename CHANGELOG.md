# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2025-02-09

First public release.

### Added

- **DAG orchestration** — dependency-aware startup and shutdown ordering with cascade failure handling
- **Four executor types** — Pixi, ROS2, Shell, and Docker in a single recipe
- **Health checks** — heartbeat, TCP, HTTP, and script-based health monitoring
- **Restart policies** — `always`, `on-failure`, and `never` with configurable max restarts and delays
- **Critical services** — mark services as critical to trigger emergency stop on failure
- **TUI** — real-time terminal interface with service list, detail view, and log viewer
- **Daemon architecture** — background daemon with IPC socket for CLI and TUI communication
- **Startup error reporting** — structured errors piped from daemon to CLI on startup failures
- **Service error visibility** — failed services show error messages in the TUI list and detail views
- **Disk and system stats** — CPU, memory, and disk usage displayed in the TUI header
- **Log management** — per-service and daemon logs with session directories
- **GPU validation** — optional GPU requirement checks before service startup
- **Process group isolation** — PGID-based process cleanup
- **Config validation** — strict YAML parsing with `deny_unknown_fields` and shell command validation
- **Python SDK** — heartbeat and status reporting from Python services
- **Rust SDK** — native client library for Rust services
- **Examples** — Pixi services, ROS2 talker/listener, ROS2 navigation stack, shell scripts, Docker
- **Documentation** — mkdocs-material site with getting started guide, configuration reference, and examples
- **CI** — GitHub Actions for testing (Linux + macOS), code coverage, docs deployment, and releases

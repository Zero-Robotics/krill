# Docker

Running a containerised service with volumes, ports, and networking.

<!-- Video placeholder -->

## Recipe

```yaml title="examples/krill-docker.yaml"
version: "1"
name: robot
log_dir: ~/.krill/logs

services:
  my-service:
    execute:
      type: docker
      image: "nginx:latest"
      volumes:
        - "./docker:/container/app"
      ports:
        - "8080:80"
      privileged: false
      network: "bridge"
```

## Running

```bash
krill up examples/krill-docker.yaml
```

Open <http://localhost:8080> to see nginx running.

## Key Concepts

- **`type: docker`** — Krill manages `docker run` for you.
- **`volumes`** — mount host paths into the container (`host:container` or `host:container:ro`).
- **`ports`** — publish container ports to the host.
- **`network`** — set the Docker network mode (`bridge`, `host`, or a named network).
- **`privileged`** — enable privileged mode for hardware access (use with care).

Docker services can be mixed with any other executor type in the same recipe — see the [ROS2 Navigation example](ros2-navigation.md) for a combined setup.

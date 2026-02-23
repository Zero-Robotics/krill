# Docker Test Example

This example demonstrates Docker executor with the `command` option.

## Services

- **nginx** — serves static HTML from a mounted volume on port 8080
- **python-hello** — runs a Python one-liner that prints and sleeps in a loop

## Running

```bash
# Make sure Docker is running
docker ps

# Start
krill up examples/krill-docker-test.yaml
```

Open http://localhost:8080 to see the nginx page.

## Testing the command option

The `python-hello` service uses the `command` field to override the container's default entrypoint:

```yaml
execute:
  type: docker
  image: "python:3.11-slim"
  command: "python -c 'import time; ...'"
```

This runs a Python script directly without needing a Dockerfile or custom image.

## Clean up

Stop the daemon:

```bash
krill down
```

This will stop and remove the containers.

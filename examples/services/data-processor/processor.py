#!/usr/bin/env python3
"""
Data Processor Service - Example Krill Service

This service processes sensor data and sends heartbeats to Krill daemon.
It demonstrates:
- Using the Krill Python SDK for heartbeat-based health checks
- Handling graceful shutdown
- Always-restart policy (configured in krill-example.yaml)
"""

import os
import signal
import sys
import time

# Add SDK to path (adjust path as needed)
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "../../../sdk/krill-python"))

from krill import KrillClient

# Global flag for graceful shutdown
shutdown_requested = False


def signal_handler(signum, frame):
    """Handle shutdown signals gracefully."""
    global shutdown_requested
    print(f"\n[data-processor] Received signal {signum}, shutting down gracefully...")
    shutdown_requested = True


def process_data(iteration):
    """Simulate data processing work."""
    print(f"[data-processor] Processing sensor data batch {iteration}")
    # Simulate work (in real service, this would read sensors, process data, etc.)
    time.sleep(0.5)
    return {"batch": iteration, "processed_items": iteration * 10}


def main():
    """Main service loop."""
    # Register signal handlers for graceful shutdown
    signal.signal(signal.SIGTERM, signal_handler)
    signal.signal(signal.SIGINT, signal_handler)

    print("[data-processor] Starting data processor service...")

    # Connect to Krill daemon
    try:
        client = KrillClient("data-processor")
        print("[data-processor] Connected to Krill daemon")
    except Exception as e:
        print(f"[data-processor] Failed to connect to Krill daemon: {e}")
        return 1

    iteration = 0

    try:
        while not shutdown_requested:
            iteration += 1

            # Simulate data processing
            result = process_data(iteration)

            # Send heartbeat to Krill daemon
            try:
                client.heartbeat_with_metadata(
                    {
                        "iteration": str(iteration),
                        "processed_items": str(result["processed_items"]),
                    }
                )
                print(f"[data-processor] Heartbeat sent (iteration {iteration})")
            except Exception as e:
                print(f"[data-processor] Failed to send heartbeat: {e}")
                # If we can't send heartbeats, the health check will fail
                # and Krill will restart us (always-restart policy)

            # Sleep between iterations
            time.sleep(1)

    except Exception as e:
        print(f"[data-processor] Error in main loop: {e}")
        return 1

    finally:
        # Clean shutdown
        print("[data-processor] Closing connection to Krill daemon")
        client.close()
        print("[data-processor] Service stopped")

    return 0


if __name__ == "__main__":
    sys.exit(main())

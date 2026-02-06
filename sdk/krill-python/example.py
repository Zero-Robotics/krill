#!/usr/bin/env python3
"""Example usage of Krill Python SDK."""

import time

from krill import KrillClient


def main():
    """Run the example."""
    try:
        # Create a client for this service
        client = KrillClient("vision-pipeline")

        print("Starting vision pipeline heartbeat loop...")

        # Main processing loop
        for i in range(10):
            # Simulate work
            time.sleep(1)

            # Send heartbeat
            if i % 3 == 0:
                # Every 3rd iteration, send with metadata
                metadata = {"frame_count": str(i * 30), "fps": "29.7"}
                client.heartbeat_with_metadata(metadata)
                print(f"Sent heartbeat with metadata (iteration {i})")
            else:
                client.heartbeat()
                print(f"Sent heartbeat (iteration {i})")

        # Simulate degraded state
        print("Simulating degraded state...")
        client.report_degraded("High latency detected")
        time.sleep(2)

        # Recover
        print("Recovered to healthy state")
        client.report_healthy()

        print("Example complete!")

        # Clean up
        client.close()

    except Exception as e:
        print(f"Error: {e}")
        return 1

    return 0


if __name__ == "__main__":
    exit(main())

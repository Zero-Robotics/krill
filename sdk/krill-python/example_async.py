#!/usr/bin/env python3
"""Example usage of Krill Python SDK (async version)."""

import asyncio

from krill import AsyncKrillClient


async def main():
    """Run the async example."""
    try:
        # Create an async client for this service
        client = await AsyncKrillClient.connect("vision-pipeline")

        print("Starting async vision pipeline heartbeat loop...")

        # Main processing loop
        for i in range(10):
            # Simulate async work
            await asyncio.sleep(1)

            # Send heartbeat
            if i % 3 == 0:
                # Every 3rd iteration, send with metadata
                metadata = {"frame_count": str(i * 30), "fps": "29.7"}
                await client.heartbeat_with_metadata(metadata)
                print(f"Sent heartbeat with metadata (iteration {i})")
            else:
                await client.heartbeat()
                print(f"Sent heartbeat (iteration {i})")

        # Simulate degraded state
        print("Simulating degraded state...")
        await client.report_degraded("High latency detected")
        await asyncio.sleep(2)

        # Recover
        print("Recovered to healthy state")
        await client.report_healthy()

        print("Example complete!")

        # Clean up
        await client.close()

    except Exception as e:
        print(f"Error: {e}")
        return 1

    return 0


if __name__ == "__main__":
    exit(asyncio.run(main()))

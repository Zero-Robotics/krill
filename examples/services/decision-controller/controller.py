#!/usr/bin/env python3
"""
Decision Controller Service - Example Krill Service

This service makes control decisions based on processed and analyzed data.
It demonstrates:
- Dependency on multiple services (data-processor AND data-analyzer)
- Critical service behavior (failure triggers emergency stop)
- Using the Krill Python SDK for health checks
- On-failure restart policy with max 3 attempts
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
    print(
        f"\n[decision-controller] Received signal {signum}, shutting down gracefully..."
    )
    shutdown_requested = True


def make_decision(iteration):
    """Simulate control decision making."""
    print(f"[decision-controller] Computing control decisions for cycle {iteration}")
    # Simulate control computation (in real service, this would compute motor commands,
    # navigation waypoints, etc.)
    time.sleep(0.3)

    # Simulate decision-making based on inputs
    if iteration % 15 == 0:
        return {"cycle": iteration, "action": "brake", "confidence": 0.85}
    elif iteration % 5 == 0:
        return {"cycle": iteration, "action": "turn", "confidence": 0.92}
    return {"cycle": iteration, "action": "forward", "confidence": 0.98}


def main():
    """Main service loop."""
    # Register signal handlers for graceful shutdown
    signal.signal(signal.SIGTERM, signal_handler)
    signal.signal(signal.SIGINT, signal_handler)

    print("[decision-controller] Starting decision controller service...")
    print(
        "[decision-controller] This is a CRITICAL service - failure triggers emergency stop"
    )
    print(
        "[decision-controller] Waiting for data-processor AND data-analyzer to be healthy..."
    )

    # Connect to Krill daemon
    try:
        client = KrillClient("decision-controller")
        print("[decision-controller] Connected to Krill daemon")
    except Exception as e:
        print(f"[decision-controller] Failed to connect to Krill daemon: {e}")
        return 1

    iteration = 0

    try:
        while not shutdown_requested:
            iteration += 1

            # Make control decisions
            decision = make_decision(iteration)

            # Send heartbeat with decision metadata
            try:
                client.heartbeat_with_metadata(
                    {
                        "cycle": str(iteration),
                        "action": decision["action"],
                        "confidence": str(decision["confidence"]),
                    }
                )
                print(
                    f"[decision-controller] ✓ Decision: {decision['action']} "
                    f"(confidence: {decision['confidence']:.2f}, cycle {iteration})"
                )
            except Exception as e:
                print(f"[decision-controller] Failed to send heartbeat: {e}")
                # As a critical service, if health checks fail after max retries,
                # Krill will trigger emergency stop of all services

            # Sleep between control cycles
            time.sleep(1)

    except Exception as e:
        print(f"[decision-controller] ERROR in main loop: {e}")
        print(
            "[decision-controller] Critical service failure - emergency stop will be triggered"
        )
        return 1

    finally:
        # Clean shutdown
        print("[decision-controller] Closing connection to Krill daemon")
        client.close()
        print("[decision-controller] Service stopped")

    return 0


if __name__ == "__main__":
    sys.exit(main())

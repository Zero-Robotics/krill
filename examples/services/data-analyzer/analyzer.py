#!/usr/bin/env python3
"""
Data Analyzer Service - Example Krill Service

This service analyzes processed data and sends heartbeats to Krill daemon.
It demonstrates:
- Dependency on data-processor (waits for it to be healthy)
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
    print(f"\n[data-analyzer] Received signal {signum}, shutting down gracefully...")
    shutdown_requested = True


def analyze_data(iteration):
    """Simulate data analysis work."""
    print(f"[data-analyzer] Analyzing data batch {iteration}")
    # Simulate analysis work (in real service, this would run ML models, compute metrics, etc.)
    time.sleep(0.7)

    # Simulate occasional analysis warnings
    if iteration % 10 == 0:
        return {"batch": iteration, "status": "warning", "anomalies": 2}
    return {"batch": iteration, "status": "normal", "anomalies": 0}


def main():
    """Main service loop."""
    # Register signal handlers for graceful shutdown
    signal.signal(signal.SIGTERM, signal_handler)
    signal.signal(signal.SIGINT, signal_handler)

    print("[data-analyzer] Starting data analyzer service...")
    print("[data-analyzer] Waiting for data-processor to be healthy...")

    # Connect to Krill daemon
    try:
        client = KrillClient("data-analyzer")
        print("[data-analyzer] Connected to Krill daemon")
    except Exception as e:
        print(f"[data-analyzer] Failed to connect to Krill daemon: {e}")
        return 1

    iteration = 0

    try:
        while not shutdown_requested:
            iteration += 1

            # Simulate data analysis
            result = analyze_data(iteration)

            # Report status based on analysis results
            try:
                if result["status"] == "warning":
                    # Report degraded status when anomalies detected
                    client.report_degraded(f"Detected {result['anomalies']} anomalies")
                    print(
                        f"[data-analyzer] ⚠️  Degraded: {result['anomalies']} anomalies in batch {iteration}"
                    )
                else:
                    # Normal healthy status
                    client.heartbeat_with_metadata(
                        {"iteration": str(iteration), "status": result["status"]}
                    )
                    print(f"[data-analyzer] ✓ Healthy (batch {iteration})")
            except Exception as e:
                print(f"[data-analyzer] Failed to send heartbeat: {e}")
                # Health check will fail, Krill will restart us (up to 3 times)

            # Sleep between iterations
            time.sleep(1)

    except Exception as e:
        print(f"[data-analyzer] Error in main loop: {e}")
        return 1

    finally:
        # Clean shutdown
        print("[data-analyzer] Closing connection to Krill daemon")
        client.close()
        print("[data-analyzer] Service stopped")

    return 0


if __name__ == "__main__":
    sys.exit(main())

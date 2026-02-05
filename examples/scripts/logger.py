#!/usr/bin/env python3
"""
Simple logger script for krill pixi testing.
Logs the current time and service name every second.
"""

import sys
import time
from datetime import datetime

def main():
    if len(sys.argv) < 2:
        print("Usage: logger.py <service_name>", file=sys.stderr)
        sys.exit(1)

    service_name = sys.argv[1]

    print(f"[{service_name}] Starting logger service", flush=True)

    try:
        while True:
            current_time = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
            print(f"[{service_name}] {current_time} - Heartbeat", flush=True)
            time.sleep(1)
    except KeyboardInterrupt:
        print(f"\n[{service_name}] Shutting down gracefully", flush=True)
        sys.exit(0)

if __name__ == "__main__":
    main()

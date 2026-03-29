"""
agent_messaging/receiver.py - Listen for incoming agent messages.

Registers agent_beta as a listener on ORE's Message Bus and polls
for incoming messages. Run this before sender.py.

Usage:
    python receiver.py
"""

import sys
import time

sys.path.insert(0, "..")
from ore_client import OreClient


def main():
    ore = OreClient()

    print("=" * 60)
    print("  ORE Agent Messenger - Receiver")
    print(f"  Listening as: agent_beta")
    print("  Press Ctrl+C to stop")
    print("=" * 60)
    print()

    # Register as a listener first (triggers channel creation on the kernel)
    ore.ipc_listen("agent_beta")
    print("  ✓ Registered on Message Bus. Waiting for messages...\n")

    try:
        while True:
            msg = ore.ipc_listen("agent_beta")

            if msg is not None:
                print("  ┌─────────────────────────────────────────")
                print(f"  │ FROM:      {msg.get('from_app', '?')}")
                print(f"  │ PAYLOAD:   {msg.get('payload', '?')}")
                print(f"  │ TIMESTAMP: {msg.get('timestamp', '?')}")
                print("  └─────────────────────────────────────────")
                print()
            else:
                sys.stdout.write("  . ")
                sys.stdout.flush()

            time.sleep(1)

    except KeyboardInterrupt:
        print("\n\n  Stopped listening. Goodbye!")
        print("=" * 60)


if __name__ == "__main__":
    main()

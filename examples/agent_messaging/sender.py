"""
agent_messaging/sender.py - Send a direct message to another agent.

Sends a message from agent_alpha to agent_beta through ORE's
Message Bus. The kernel checks agent_alpha's manifest to verify
that agent_beta is in its allowed_agent_targets.

Usage:
    python sender.py
    python sender.py --message "Analyze the latest data"
"""

import sys
import argparse

sys.path.insert(0, "..")
from ore_client import OreClient


def main():
    parser = argparse.ArgumentParser(description="ORE Message Sender")
    parser.add_argument(
        "--message",
        default="Hey Beta, can you look into the latest Rust RFC on async closures?",
        help="Message payload to send",
    )
    parser.add_argument("--to", default="agent_beta", help="Target agent ID")
    args = parser.parse_args()

    ore = OreClient()

    print("=" * 60)
    print("  ORE Agent Messenger - Sender")
    print(f"  From: agent_alpha")
    print(f"  To:   {args.to}")
    print("=" * 60)

    print(f"\n  Sending message: \"{args.message}\"")
    print()

    result = ore.ipc_send(
        from_app="agent_alpha",
        to_app=args.to,
        payload=args.message,
    )

    print(f"  Kernel Response: {result}")

    if "SUCCESS" in result:
        print("\n  ✓ Message delivered to the agent's broadcast channel.")
        print(f"  The receiver can pick it up via GET /ipc/listen/{args.to}")
    else:
        print("\n  ✗ Message was blocked by the IPC firewall.")
        print("  Check that agent_beta is in agent_alpha's allowed_agent_targets.")

    print("=" * 60)


if __name__ == "__main__":
    main()

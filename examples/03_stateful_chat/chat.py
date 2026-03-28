"""
03_stateful_chat/chat.py — Multi-turn conversations with memory.

Unlike the Simple Bot, this chat uses ORE's /ask route which
automatically pages conversation history to/from the SSD.
Your conversation survives kernel restarts.

Usage:
    python chat.py
"""

import sys

sys.path.insert(0, "..")
from ore_client import OreClient


def main():
    ore = OreClient()

    print("=" * 60)
    print("  ORE Stateful Chat")
    print("  Powered by SSD Pager (swap/openclaw.json)")
    print()
    print("  Your conversation is saved to disk automatically.")
    print("  It survives kernel restarts.")
    print()
    print("  Commands:")
    print("    /clear  — Wipe conversation history")
    print("    /quit   — Exit")
    print("=" * 60)

    # Check kernel health
    try:
        status = ore.health()
        print(f"  Kernel: {status}")
    except Exception as e:
        print(f"  ERROR: Cannot reach ORE Kernel — {e}")
        sys.exit(1)

    print()

    while True:
        try:
            user_input = input("You > ").strip()
        except (KeyboardInterrupt, EOFError):
            print("\nGoodbye!")
            break

        if not user_input:
            continue

        if user_input.lower() == "/quit":
            print("Goodbye!")
            break

        if user_input.lower() == "/clear":
            result = ore.clear("openclaw")
            print(f"  [{result}]")
            print("  Conversation history wiped.\n")
            continue

        # The /ask route automatically handles:
        #   1. Page-in previous history from swap/openclaw.json
        #   2. Run through the Context Firewall
        #   3. Append this message to history
        #   4. Generate response with full context
        #   5. Page-out updated history to swap/openclaw.json
        try:
            response = ore.ask(user_input)
            print(f"Bot > {response}\n")
        except Exception as e:
            print(f"  [ERROR] {e}\n")


if __name__ == "__main__":
    main()

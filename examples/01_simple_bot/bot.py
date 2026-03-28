"""
01_simple_bot/bot.py — The "Hello World" of ORE.

A minimal terminal chatbot that sends prompts to the ORE Kernel
and streams the AI response back in real-time.

Usage:
    python bot.py
    python bot.py --model llama3.2:1b
"""

import sys
import argparse

sys.path.insert(0, "..")
from ore_client import OreClient


def main():
    parser = argparse.ArgumentParser(description="ORE Simple Bot")
    parser.add_argument("--model", default="qwen2.5:0.5b", help="Model to use")
    args = parser.parse_args()

    ore = OreClient()

    print("=" * 50)
    print("  ORE Simple Bot")
    print(f"  Model: {args.model}")
    print("  Type 'quit' to exit")
    print("=" * 50)

    # Verify the kernel is alive
    try:
        status = ore.health()
        print(f"  Kernel: {status}")
    except Exception as e:
        print(f"  ERROR: Cannot reach ORE Kernel — {e}")
        print("  Start it with: cargo run --release -p ore-server")
        sys.exit(1)

    print("=" * 50)
    print()

    while True:
        try:
            user_input = input("You > ").strip()
        except (KeyboardInterrupt, EOFError):
            print("\nGoodbye!")
            break

        if not user_input:
            continue
        if user_input.lower() in ("quit", "exit"):
            print("Goodbye!")
            break

        print(f"Bot > ", end="", flush=True)
        try:
            ore.run(args.model, user_input, stream=True)
        except Exception as e:
            print(f"\n[ERROR] {e}")

        print()


if __name__ == "__main__":
    main()

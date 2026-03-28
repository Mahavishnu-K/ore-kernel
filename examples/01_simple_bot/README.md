# 01 — Simple Bot

> The "Hello World" of ORE. A 30-line Python script that proves how easy it is to build an AI app when ORE handles the heavy lifting.

## What It Does

A terminal REPL that takes your input, sends it to the ORE Kernel via `POST /run`, and streams the response back in real-time. The kernel handles security (firewall), scheduling (GPU mutex), and rate limiting automatically.

## Setup

```bash
# 1. Make sure the kernel is running
cargo run --release -p ore-server

# 2. Make sure you have a model installed
ore pull qwen2.5:0.5b

# 3. Run the bot
cd examples/01_simple_bot
python bot.py
```

## Architecture

```
┌──────────────┐     POST /run      ┌──────────────────┐
│   bot.py     │ ──────────────────▶ │   ORE Kernel     │
│  (terminal)  │ ◀────────────────── │  · Firewall      │
│              │   streamed tokens   │  · Scheduler     │
└──────────────┘                     │  · Rate Limiter  │
                                     └──────────────────┘
```

## Files

| File | Purpose |
|---|---|
| `bot.py` | The bot script — ~30 lines of Python |
| `simple_bot.toml` | Manifest for the `terminal_user` identity this bot uses |

> **Note:** The `/run` route uses `terminal_user` as its app identity. The default `terminal_user.toml` manifest is already installed in `manifests/`. No extra setup needed.

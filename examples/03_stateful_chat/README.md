# 03 — Stateful Chat

> Multi-turn conversations that survive kernel restarts. ORE's SSD Pager remembers context automatically.

## What It Does

A conversational chatbot that maintains history across multiple exchanges. Unlike the Simple Bot (which is stateless), this example uses ORE's `GET /ask` route which automatically:

1. **Pages in** the agent's previous chat history from SSD
2. **Appends** the new message
3. **Runs inference** with the full context
4. **Pages out** the updated history back to SSD

Your conversation survives kernel restarts. No database required.

## Setup

```bash
# 1. Kernel must be running
cargo run --release -p ore-server

# 2. Run the chat
cd examples/03_stateful_chat
python chat.py
```

## Architecture

```
Turn 1:
  chat.py  ──▶  GET /ask/hello  ──▶  Kernel
                                      │
                                      ├── page_in("openclaw")  → [] (empty)
                                      ├── firewall(prompt)
                                      ├── scheduler.request_gpu()
                                      ├── driver.generate_text(history=[user: "hello"])
                                      └── page_out("openclaw") → swap/openclaw.json

Turn 2:
  chat.py  ──▶  GET /ask/what_did_i_say  ──▶  Kernel
                                               │
                                               ├── page_in("openclaw")  → [{user: "hello"}, {bot: "..."}]
                                               ├── firewall(prompt)
                                               ├── generate_text(history=[...full context...])
                                               └── page_out("openclaw") → swap/openclaw.json (updated)
```

## Files

| File | Purpose |
|---|---|
| `chat.py` | Multi-turn chat using the `/ask` route |

> **Note:** The `/ask` route uses the `openclaw` manifest identity, which has `stateful_paging = true`. This is already configured in `manifests/openclaw.toml`.

## Key Difference from Simple Bot

| | Simple Bot (`/run`) | Stateful Chat (`/ask`) |
|---|---|---|
| **History** | None — each prompt is independent | Full conversation history from SSD |
| **Manifest** | `terminal_user` | `openclaw` |
| **Paging** | Disabled | Automatic page-in/page-out |
| **Use Case** | One-shot questions | Long conversations |

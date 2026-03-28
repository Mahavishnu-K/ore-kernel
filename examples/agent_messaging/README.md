# 04 — Agent Messaging

> Direct agent-to-agent communication through ORE's Message Bus.

## What It Does

Two Python scripts simulate two agents communicating in real-time through ORE's IPC Message Bus:

1. **`receiver.py`** registers as a listener and polls for incoming messages
2. **`sender.py`** sends a direct message to the receiver through ORE

The kernel enforces that the sender's manifest lists the receiver in `allowed_agent_targets`. Unauthorized messages are blocked.

## Setup

```bash
# 1. Install the example manifests
cp examples/agent_messaging/manifests/*.toml manifests/

# 2. Restart the kernel
cargo run --release -p ore-server

# 3. Terminal A — start the receiver (polls for messages)
cd examples/agent_messaging
python receiver.py

# 4. Terminal B — send a message
cd examples/agent_messaging
python sender.py --message "Hello from sender!"
```

## Architecture

```
┌───────────────┐   POST /ipc/send    ┌──────────────────────────┐
│  sender.py    │ ────────────────────▶│      ORE KERNEL          │
│  (agent_alpha)│                      │                          │
└───────────────┘                      │  ┌────────────────────┐  │
                                       │  │    Message Bus     │  │
                                       │  │                    │  │
                                       │  │  DashMap<String,   │  │
                                       │  │    broadcast::Tx>  │  │
                                       │  └────────────────────┘  │
┌───────────────┐  GET /ipc/listen     │                          │
│  receiver.py  │ ◀────────────────────│  Manifest check:         │
│  (agent_beta) │                      │  agent_alpha → agent_beta│
└───────────────┘                      │  ✓ Allowed               │
                                       └──────────────────────────┘
```

## Files

| File | Purpose |
|---|---|
| `sender.py` | Sends messages as `agent_alpha` |
| `receiver.py` | Listens for messages as `agent_beta` |
| `manifests/agent_alpha.toml` | Sender manifest — lists `agent_beta` as allowed target |
| `manifests/agent_beta.toml` | Receiver manifest — registered so it can listen |

## IPC Firewall in Action

Agent Alpha's manifest explicitly allows messaging Agent Beta:

```toml
[ipc]
allowed_agent_targets = ["agent_beta"]
```

If you try to send a message to an agent not in your `allowed_agent_targets`, the kernel blocks it:

```
KERNEL ALERT: IPC Target 'unauthorized_agent' not in allowed_agent_targets manifest.
```

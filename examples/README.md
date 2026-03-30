# ORE Examples

> Real, runnable applications built on top of the ORE Kernel.
> Each example is a standalone Python script that talks to ORE over HTTP.

## Prerequisites

1. **ORE Kernel running** - `cargo run --release -p ore-server` in a separate terminal
2. **Python 3.8+** with `requests` installed: `pip install requests`
3. **A model pulled** - `ore pull llama3.2:1b` (or `qwen2.5:0.5b`)
4. **System embedder** (for IPC examples) - `ore pull system-embedder`

## Examples

| # | Name | Features Demonstrated |
|---|---|---|
| 01 | [**Simple Bot**](./simple_bot/) | Basic inference via `POST /run` |
| 02 | [**Semantic Swarm**](./semantic_swarm/) | Semantic Bus: `/ipc/share` + `/ipc/search` |
| 03 | [**Stateful Chat**](./stateful_chat/) | SSD Pager: multi-turn conversations via `GET /ask` |
| 04 | [**Agent Messaging**](./agent_messaging/) | Direct agent IPC: `/ipc/send` + `/ipc/listen` |
| 05 | [**Firewall Demo**](./firewall_demo/) | Context Firewall: injection blocking + PII redaction |
| 06 | [**Multi-Agent Pipeline**](./multi_agent_pipeline/) | Full swarm: 3 agents chained via Semantic Bus |

## Quick Start

```bash
# Terminal 1: Boot the kernel
cargo run --release -p ore-server

# Terminal 2: Run any example
cd examples/simple_bot
python bot.py
```

## Shared Utility

All examples import [`ore_client.py`](./ore_client.py) - a lightweight Python wrapper around ORE's HTTP API. It handles token authentication, error formatting, and streaming automatically.

## Manifest Installation

Each example includes the `.toml` manifests it needs. Copy them to the `manifests/` directory and restart the kernel before running:

```bash
# Example: install manifests for the Semantic Swarm
cp examples/semantic_swarm/manifests/*.toml manifests/
```

Then reboot the kernel to load the new manifests.

# 02 — Semantic Swarm

> The "killer feature" showcase. Two agents share memory through ORE's Semantic Bus — no external vector database required.

## What It Does

1. **`scraper_agent.py`** downloads content from a topic (e.g., "Rust programming language") and pushes it into the ORE Semantic Bus via `POST /ipc/share`
2. **`writer_agent.py`** searches that shared memory via `POST /ipc/search`, retrieves the most relevant paragraphs, and asks ORE to write an essay using that context

The two scripts share knowledge through a named **semantic pipe** (`research`) — ORE handles the embedding, vector storage, and cosine similarity search entirely in memory.

## Setup

```bash
# 1. Install the example manifests
cp examples/02_semantic_swarm/manifests/*.toml manifests/

# 2. Restart the kernel to pick up the new manifests
cargo run --release -p ore-server

# 3. Make sure you have the system embedder installed
ore pull system-embedder
ore pull qwen2.5:0.5b

# 4. Run the scraper first (populates the Semantic Bus)
cd examples/02_semantic_swarm
python scraper_agent.py

# 5. Then run the writer (queries the Semantic Bus and generates text)
python writer_agent.py
```

## Architecture

```
┌───────────────────┐                      ┌───────────────────┐
│  scraper_agent.py │                      │  writer_agent.py  │
│                   │                      │                   │
│  1. Fetch content │                      │  3. Search pipe   │
│  2. POST share    │                      │  4. POST /run     │
└────────┬──────────┘                      └────────┬──────────┘
         │                                          │
         │  POST /ipc/share                         │  POST /ipc/search
         │  (text → chunks → embeddings)            │  (query → cosine similarity)
         ▼                                          ▼
╔═══════════════════════════════════════════════════════════════╗
║                    ORE KERNEL                                 ║
║                                                               ║
║   ┌───────────────────────────────────────────────────────┐   ║
║   │  Semantic Bus                                         │   ║
║   │  Pipe: "research"                                     │   ║
║   │  ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐         │   ║
║   │  │chunk 1 │ │chunk 2 │ │chunk 3 │ │chunk N │  ...     │   ║
║   │  │vec[384]│ │vec[384]│ │vec[384]│ │vec[384]│         │   ║
║   │  └────────┘ └────────┘ └────────┘ └────────┘         │   ║
║   └───────────────────────────────────────────────────────┘   ║
╚═══════════════════════════════════════════════════════════════╝
```

## Files

| File | Purpose |
|---|---|
| `scraper_agent.py` | Fetches content and pushes it to the `research` pipe |
| `writer_agent.py` | Searches the pipe and asks ORE to write based on the results |
| `manifests/scraper_agent.toml` | Manifest granting access to the `research` pipe |
| `manifests/swarm_writer.toml` | Manifest granting access to the `research` pipe |

## Why This Matters

Most "multi-agent" demos require you to spin up a separate vector database (Pinecone, Chroma, Weaviate). With ORE, the vector memory is **built into the kernel**. Two Python scripts, zero infrastructure.

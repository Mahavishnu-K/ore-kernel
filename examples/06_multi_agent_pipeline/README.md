# 06 — Multi-Agent Pipeline

> The full swarm showcase. Three agents chained together via the Semantic Bus — researcher → analyst → reporter.

## What It Does

A three-stage AI pipeline where each agent has a specialized role:

1. **Researcher** (`researcher.py`) — Gathers raw knowledge about a topic and pushes it to the `pipeline_data` semantic pipe
2. **Analyst** (`analyst.py`) — Searches the `pipeline_data` pipe, asks the LLM to extract key insights, and pushes the analysis to the `pipeline_analysis` pipe
3. **Reporter** (`reporter.py`) — Searches the `pipeline_analysis` pipe and generates a final executive summary

Each agent only has access to the pipes it needs. The researcher can't read the analysis. The reporter can't write raw data. This is **manifest-enforced separation of concerns**.

## Setup

```bash
# 1. Install the example manifests
cp examples/06_multi_agent_pipeline/manifests/*.toml manifests/

# 2. Restart the kernel
cargo run --release -p ore-server

# 3. Make sure embedder + model are installed
ore pull system-embedder
ore pull qwen2.5:0.5b

# 4. Run the pipeline stages in order
cd examples/06_multi_agent_pipeline
python researcher.py
python analyst.py
python reporter.py
```

## Architecture

```
┌─────────────┐  write   ┌─────────────────┐  write   ┌─────────────────┐
│ researcher  │ ────────▶ │  pipeline_data  │          │pipeline_analysis│
│             │           │  (semantic pipe)│          │ (semantic pipe) │
└─────────────┘           └───────┬─────────┘          └───────┬─────────┘
                                  │ read                       │ read
                                  ▼                            ▼
                          ┌─────────────┐  write       ┌─────────────┐
                          │   analyst   │ ────────────▶│   reporter  │
                          │             │  to analysis ││             │
                          └─────────────┘              └─────────────┘
                                                              │
                                                              ▼
                                                       Final Report
```

## Manifest Permissions

Each agent has carefully scoped pipe access:

| Agent | Read Pipes | Write Pipes |
|---|---|---|
| `pipeline_researcher` | — | `pipeline_data` |
| `pipeline_analyst` | `pipeline_data` | `pipeline_analysis` |
| `pipeline_reporter` | `pipeline_analysis` | — |

The researcher **cannot** read analysis results. The reporter **cannot** inject raw data. ORE enforces this at the kernel level.

## Files

| File | Purpose |
|---|---|
| `researcher.py` | Gathers knowledge → pushes to `pipeline_data` |
| `analyst.py` | Reads `pipeline_data` → analyzes → pushes to `pipeline_analysis` |
| `reporter.py` | Reads `pipeline_analysis` → generates final report |
| `manifests/pipeline_researcher.toml` | Write access to `pipeline_data` only |
| `manifests/pipeline_analyst.toml` | Read `pipeline_data` + write `pipeline_analysis` |
| `manifests/pipeline_reporter.toml` | Read `pipeline_analysis` only |

## Why This Matters

This demonstrates three ORE differentiators simultaneously:

1. **Built-in vector memory** — No Pinecone, no Chroma, no Weaviate
2. **Manifest-enforced data isolation** — Agents can only touch their designated pipes
3. **Agent chaining** — Complex pipelines from simple Python scripts

"""
multi_agent_pipeline/researcher.py — Stage 1: Knowledge Gathering.

Collects raw knowledge about a topic and pushes it into the
'pipeline_data' semantic pipe for downstream agents.

Usage:
    python researcher.py
    python researcher.py --topic "WebAssembly"
"""

import sys
import argparse

sys.path.insert(0, "..")
from ore_client import OreClient

# ─── Research Corpus ─────────────────────────────────────────────

CORPUS = {
    "WebAssembly": """
WebAssembly (abbreviated Wasm) is a binary instruction format for a stack-based virtual
machine. Wasm is designed as a portable compilation target for programming languages,
enabling deployment on the web for client and server applications. WebAssembly aims to
execute at native speed by taking advantage of common hardware capabilities available on
a wide range of platforms.

WebAssembly is an open standard developed by the W3C WebAssembly Community Group. It defines
a portable binary-code format and a corresponding text format for executable programs, as
well as interfaces for facilitating interactions between such programs and their host
environment. The main goal is to be a compilation target for high-level languages like C,
C++, Rust, and Go.

Performance characteristics of WebAssembly are notable. It uses a compact binary format
that is size-efficient and loads fast. The format is designed to be parsed and validated
quickly. WebAssembly code runs in a sandboxed execution environment. It does not require
garbage collection because it operates with linear memory that the host manages. The
instruction set is designed around structured control flow, which prevents many classes of
security vulnerabilities.

Security in WebAssembly is achieved through several mechanisms. Code runs in a sandboxed
environment with no direct access to the host system. Memory access is bounds-checked.
There are no ambient permissions — all system interactions must go through explicitly
imported host functions. This makes WebAssembly suitable for running untrusted code safely.
The capability-based security model means programs can only access resources that are
explicitly granted to them through the import mechanism.

WASI (WebAssembly System Interface) extends WebAssembly beyond the browser. WASI provides
a standardized set of system interfaces that allow WebAssembly modules to interact with the
operating system in a secure, portable way. This includes file system access, network
sockets, clocks, and random number generation. WASI uses a capability-based security model
where a WebAssembly module can only access the resources that are explicitly passed to it.

The WebAssembly Component Model is the next evolution. Components are WebAssembly modules
that communicate through well-defined interfaces using WIT (WebAssembly Interface Types).
Components can be composed together, linked at build time or runtime, and can interact
across language boundaries. A Rust component can call a Python component which calls a
Go component, all through type-safe interfaces.

Real-world adoption of WebAssembly includes Figma (design tool), Google Earth, AutoCAD,
and many cloud computing platforms. Cloudflare Workers, Fastly Compute@Edge, and Fermyon
Spin all use WebAssembly for serverless edge computing. Docker has added WebAssembly
support, allowing Wasm modules to run alongside traditional containers.
""",
    "Rust async runtime": """
Rust's async runtime ecosystem is built on the Future trait and the async/await syntax.
Unlike languages like Go or Java, Rust does not include a built-in async runtime. Instead,
the runtime is provided by third-party crates, with Tokio being the most widely used.

The Future trait in Rust is a zero-cost abstraction. A Future represents a value that may
not be available yet. The trait has a single method called poll that returns either
Poll::Ready with the final value or Poll::Pending to indicate the value is not yet available.
Futures in Rust are lazy — they do nothing until polled. This is different from JavaScript
Promises which start executing immediately.

Tokio is a multi-threaded async runtime for Rust. It provides an event loop, I/O drivers
for networking and file system operations, timers, and synchronization primitives. Tokio
uses a work-stealing scheduler that distributes tasks across a thread pool. When a task
blocks on I/O, the thread can execute other ready tasks instead of sitting idle.

Key Tokio synchronization primitives include Mutex (async-aware mutual exclusion), Semaphore
(counting semaphore for limiting concurrent access), broadcast channels (multiple producer,
multiple consumer), mpsc channels (multiple producer, single consumer), and oneshot channels
(single-use communication). The Notify primitive allows tasks to wait for a signal.

The async/await syntax in Rust desugars into state machines at compile time. Each .await
point becomes a state in the generated state machine. This means async Rust has zero
allocation overhead for the state machine itself — the compiler generates exactly the code
needed. The downside is that async functions cannot be recursive without boxing.

Structured concurrency in Tokio is achieved through JoinHandle and JoinSet. When you
spawn a task with tokio::spawn, you get a JoinHandle that can be awaited to get the
task's result. JoinSet allows you to spawn a collection of tasks and await them as a
group. Task cancellation happens automatically when a JoinHandle is dropped.

Error handling in async Rust follows the same patterns as synchronous Rust. The Result type
and the question mark operator work identically in async functions. The main challenge is
that error types must be Send when tasks are spawned across threads.
""",
}


def main():
    parser = argparse.ArgumentParser(description="Pipeline Stage 1: Researcher")
    parser.add_argument(
        "--topic",
        default="WebAssembly",
        choices=list(CORPUS.keys()),
        help="Research topic",
    )
    args = parser.parse_args()

    ore = OreClient()

    print()
    print("╔" + "═" * 58 + "╗")
    print("║" + "  PIPELINE STAGE 1 — RESEARCHER".center(58) + "║")
    print("╚" + "═" * 58 + "╝")
    print()

    content = CORPUS[args.topic]
    word_count = len(content.split())

    print(f"  Topic:      {args.topic}")
    print(f"  Words:      {word_count}")
    print(f"  Target:     Semantic Bus → 'pipeline_data'")
    print(f"  Chunking:   80 words, 15 overlap")
    print()

    print("  Pushing knowledge to Semantic Bus...")

    result = ore.ipc_share(
        source_app="pipeline_researcher",
        target_pipe="pipeline_data",
        knowledge_text=content,
        chunk_size=80,
        chunk_overlap=15,
    )

    print(f"  Kernel: {result}")
    print()
    print("  ✓ Stage 1 complete. Raw knowledge is now in the pipeline.")
    print("  Next: Run analyst.py (Stage 2)")
    print()


if __name__ == "__main__":
    main()

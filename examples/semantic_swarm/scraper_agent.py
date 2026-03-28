"""
semantic_swarm/scraper_agent.py — The knowledge gatherer.

This agent fetches content about a topic and pushes it into ORE's
Semantic Bus. The content is automatically chunked, embedded, and
stored in the "research" pipe for other agents to search.

Usage:
    python scraper_agent.py
    python scraper_agent.py --topic "Rust ownership and borrowing"
"""

import sys
import argparse

sys.path.insert(0, "..")
from ore_client import OreClient

# ─── Sample Knowledge Base ──────────────────────────────────────
# In production, you'd fetch this from a URL, PDF, or database.
# We hardcode rich content here so the example runs without internet.

KNOWLEDGE = {
    "Rust ownership and borrowing": """
Rust's ownership system is a set of rules that the compiler checks at compile time.
It does not slow down your program while running. The ownership system has three rules:
Each value in Rust has an owner. There can only be one owner at a time. When the owner
goes out of scope, the value will be dropped.

Borrowing in Rust allows you to reference data without taking ownership of it. References
are created using the ampersand symbol. There are two types of references: immutable
references (shared references) created with &T, and mutable references created with &mut T.
You can have either one mutable reference or any number of immutable references to a
particular piece of data in a particular scope. References must always be valid.

The borrow checker is the part of the Rust compiler that enforces these rules. It tracks
the lifetimes of all references and ensures that no reference outlives the data it points
to. This prevents dangling references, use-after-free bugs, and data races at compile time
rather than at runtime. This is what makes Rust memory-safe without a garbage collector.

Move semantics in Rust mean that assigning a value to another variable transfers ownership.
After a move, the original variable can no longer be used. This applies to types that do
not implement the Copy trait, such as String, Vec, and other heap-allocated types. Types
that implement Copy (like integers, booleans, and floating point numbers) are copied instead
of moved.

Lifetimes are Rust's way of ensuring that references are valid for as long as they are used.
Every reference in Rust has a lifetime, which is the scope for which that reference is valid.
Most of the time, lifetimes are implicit and inferred by the compiler. When the compiler
cannot infer the lifetimes, you must annotate them explicitly using lifetime parameters.
The lifetime annotation syntax uses an apostrophe followed by a name, like 'a.
""",
    "Operating system scheduling": """
An operating system scheduler determines the order in which processes execute on the CPU.
The scheduler makes decisions about which process to run next based on a scheduling algorithm.
Common algorithms include First-Come First-Served (FCFS), Shortest Job Next (SJN),
Round Robin (RR), and Priority Scheduling.

In preemptive scheduling, the operating system can interrupt a currently running process
and move it to the ready queue. This is used in most modern operating systems to ensure
fairness and responsiveness. Time slicing divides CPU time into small intervals called
time quanta and switches between processes at each interval.

A semaphore is a synchronization primitive used to control access to shared resources.
A counting semaphore allows a specified number of threads to access a resource simultaneously.
A binary semaphore (or mutex) restricts access to a single thread at a time. The semaphore
maintains a counter that is decremented when a thread acquires the semaphore and incremented
when a thread releases it. If the counter reaches zero, subsequent threads are blocked.

Context switching is the process of saving the state of a currently running process and
loading the state of the next process to run. The state includes the program counter,
registers, and memory mappings. Context switches are computationally expensive because
they require saving and loading state, flushing caches, and performing TLB invalidations.
Modern operating systems minimize context switches through techniques like batching and
cooperative scheduling.

Virtual memory allows operating systems to use disk storage as an extension of RAM. Pages
of memory that are not actively used can be swapped to disk (paged out), freeing physical
RAM for other processes. When a paged-out memory page is accessed, the OS triggers a page
fault, loads the page back into RAM (paged in), and resumes the process.
""",
}


def main():
    parser = argparse.ArgumentParser(description="ORE Scraper Agent")
    parser.add_argument(
        "--topic",
        default="Rust ownership and borrowing",
        choices=list(KNOWLEDGE.keys()),
        help="Topic to push to the Semantic Bus",
    )
    args = parser.parse_args()

    ore = OreClient()

    print("=" * 60)
    print("  ORE Scraper Agent")
    print(f"  Topic: {args.topic}")
    print("=" * 60)

    content = KNOWLEDGE[args.topic]
    word_count = len(content.split())
    print(f"\n  Loaded {word_count} words of knowledge.")
    print(f"  Pushing to Semantic Bus pipe: 'research'")
    print(f"  Chunking: 50 words per chunk, 10 word overlap\n")

    result = ore.ipc_share(
        source_app="scraper_agent",
        target_pipe="research",
        knowledge_text=content,
        chunk_size=50,
        chunk_overlap=10,
    )

    print(f"  Kernel Response: {result}")
    print()
    print("  ✓ Knowledge is now searchable by any agent with")
    print("    'research' in their allowed_semantic_pipes.")
    print()
    print("  Next: Run writer_agent.py to search this knowledge.")
    print("=" * 60)


if __name__ == "__main__":
    main()

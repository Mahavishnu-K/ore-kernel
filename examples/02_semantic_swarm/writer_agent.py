"""
02_semantic_swarm/writer_agent.py — The knowledge consumer.

This agent searches ORE's Semantic Bus for relevant context,
then asks the LLM to write a response grounded in that knowledge.

Run scraper_agent.py first to populate the Semantic Bus.

Usage:
    python writer_agent.py
    python writer_agent.py --query "How does the borrow checker work?"
"""

import sys
import argparse

sys.path.insert(0, "..")
from ore_client import OreClient


def main():
    parser = argparse.ArgumentParser(description="ORE Writer Agent")
    parser.add_argument(
        "--query",
        default="How does Rust prevent memory bugs without a garbage collector?",
        help="Question to research and answer",
    )
    parser.add_argument("--model", default="qwen2.5:0.5b", help="Model to use")
    parser.add_argument("--top-k", type=int, default=3, help="Number of context chunks")
    args = parser.parse_args()

    ore = OreClient()

    print("=" * 60)
    print("  ORE Writer Agent")
    print(f"  Query: {args.query}")
    print(f"  Model: {args.model}")
    print("=" * 60)

    # Step 1: Search the Semantic Bus for relevant context
    print("\n  [1/3] Searching Semantic Bus (pipe: 'research')...")

    results = ore.ipc_search(
        source_app="swarm_writer",
        target_pipe="research",
        query=args.query,
        top_k=args.top_k,
    )

    if not results:
        print("  No results found. Run scraper_agent.py first!")
        sys.exit(1)

    print(f"  Found {len(results)} relevant chunks:\n")
    for i, chunk in enumerate(results, 1):
        preview = chunk[:80].replace("\n", " ")
        print(f"    [{i}] {preview}...")

    # Step 2: Build a grounded prompt with the retrieved context
    print("\n  [2/3] Building context-grounded prompt...")

    context = "\n\n".join(results)
    grounded_prompt = (
        f"You are a technical writer. Based ONLY on the following research context, "
        f"write a clear, concise answer to the question.\n\n"
        f"--- RESEARCH CONTEXT ---\n{context}\n--- END CONTEXT ---\n\n"
        f"Question: {args.query}\n\n"
        f"Answer:"
    )

    # Step 3: Generate the response
    print("  [3/3] Generating response...\n")
    print("-" * 60)

    ore.run(args.model, grounded_prompt, stream=True)

    print("-" * 60)
    print("\n  ✓ Response generated from Semantic Bus knowledge.")
    print("  No external vector database was used.")
    print("=" * 60)


if __name__ == "__main__":
    main()

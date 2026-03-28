"""
06_multi_agent_pipeline/analyst.py — Stage 2: Analysis.

Searches the 'pipeline_data' pipe for raw knowledge, asks the LLM
to extract key insights, and pushes the analysis to 'pipeline_analysis'.

Run researcher.py first.

Usage:
    python analyst.py
    python analyst.py --query "What are the security properties?"
"""

import sys
import argparse

sys.path.insert(0, "..")
from ore_client import OreClient


def main():
    parser = argparse.ArgumentParser(description="Pipeline Stage 2: Analyst")
    parser.add_argument(
        "--query",
        default="What are the key technical properties and security features?",
        help="Analysis query",
    )
    parser.add_argument("--model", default="qwen2.5:0.5b", help="Model to use")
    parser.add_argument("--top-k", type=int, default=5, help="Number of chunks to retrieve")
    args = parser.parse_args()

    ore = OreClient()

    print()
    print("╔" + "═" * 58 + "╗")
    print("║" + "  PIPELINE STAGE 2 — ANALYST".center(58) + "║")
    print("╚" + "═" * 58 + "╝")
    print()

    # Step 1: Search the raw data pipe
    print(f"  Query:  {args.query}")
    print(f"  Source: Semantic Bus → 'pipeline_data'")
    print()
    print("  [1/3] Searching raw knowledge base...")

    results = ore.ipc_search(
        source_app="pipeline_analyst",
        target_pipe="pipeline_data",
        query=args.query,
        top_k=args.top_k,
    )

    if not results:
        print("  No data found. Run researcher.py first!")
        sys.exit(1)

    print(f"  Retrieved {len(results)} relevant chunks.\n")

    for i, chunk in enumerate(results, 1):
        preview = chunk[:70].replace("\n", " ").strip()
        print(f"    [{i}] {preview}...")

    # Step 2: Ask the LLM to analyze
    print("\n  [2/3] Generating analysis with LLM...")

    context = "\n\n".join(results)
    analysis_prompt = (
        f"You are a senior technical analyst. Based on the following research data, "
        f"extract the 3-5 most important technical insights. Be specific and concise. "
        f"Format as numbered bullet points.\n\n"
        f"--- RAW DATA ---\n{context}\n--- END DATA ---\n\n"
        f"Analysis query: {args.query}\n\n"
        f"Key Insights:"
    )

    analysis = ore.run(args.model, analysis_prompt)

    print("\n  Analysis generated:")
    print("  " + "-" * 50)
    for line in analysis.strip().split("\n"):
        print(f"  {line}")
    print("  " + "-" * 50)

    # Step 3: Push the analysis to the next pipe
    print("\n  [3/3] Pushing analysis to 'pipeline_analysis'...")

    result = ore.ipc_share(
        source_app="pipeline_analyst",
        target_pipe="pipeline_analysis",
        knowledge_text=analysis,
        chunk_size=60,
        chunk_overlap=10,
    )

    print(f"  Kernel: {result}")
    print()
    print("  ✓ Stage 2 complete. Analysis is ready for the reporter.")
    print("  Next: Run reporter.py (Stage 3)")
    print()


if __name__ == "__main__":
    main()

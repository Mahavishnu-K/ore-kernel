"""
multi_agent_pipeline/reporter.py - Stage 3: Final Report.

Searches the 'pipeline_analysis' pipe for processed insights and
generates a polished executive summary.

Run researcher.py and analyst.py first.

Usage:
    python reporter.py
"""

import sys
import argparse

sys.path.insert(0, "..")
from ore_client import OreClient


def main():
    parser = argparse.ArgumentParser(description="Pipeline Stage 3: Reporter")
    parser.add_argument("--model", default="qwen2.5:0.5b", help="Model to use")
    args = parser.parse_args()

    ore = OreClient()

    print()
    print("╔" + "═" * 58 + "╗")
    print("║" + "  PIPELINE STAGE 3 - REPORTER".center(58) + "║")
    print("╚" + "═" * 58 + "╝")
    print()

    # Step 1: Retrieve the analysis from the previous stage
    print("  [1/2] Retrieving analysis from 'pipeline_analysis'...")

    results = ore.ipc_search(
        source_app="pipeline_reporter",
        target_pipe="pipeline_analysis",
        query="key insights and technical conclusions",
        top_k=5,
    )

    if not results:
        print("  No analysis found. Run analyst.py first!")
        sys.exit(1)

    context_chunks = [r['text'] for r in results]
    context = "\n\n".join(context_chunks)

    print(f"  Found {len(results)} relevant chunks:\n")
    
    for i, r in enumerate(results, 1):
        preview = r['text'][:60].replace("\n", " ")
        print(f"    [{i}] [Score: {r['score']:.2f}] {preview}...")

    print("  [2/2] Generating executive summary...\n")
    report_prompt = (
        f"You are a technical report writer. Based on the following analysis, "
        f"write a concise executive summary (3-4 paragraphs). "
        f"The tone should be professional and suitable for a technical audience. "
        f"Include a 'Key Takeaways' section with 3 bullet points at the end.\n\n"
        f"--- ANALYST FINDINGS ---\n{context}\n--- END FINDINGS ---\n\n"
        f"Executive Summary:"
    )

    print("╔" + "═" * 58 + "╗")
    print("║" + "  EXECUTIVE SUMMARY".center(58) + "║")
    print("╠" + "═" * 58 + "╣")
    print("║" + " " * 58 + "║")

    report = ore.run(args.model, report_prompt, stream=True)

    print()
    print("╚" + "═" * 58 + "╝")

    print()
    print("  ✓ Pipeline complete!")
    print()
    print("  Data flow:")
    print("    researcher.py → [pipeline_data] → analyst.py → [pipeline_analysis] → reporter.py")
    print()
    print("  Three agents. Two semantic pipes. Zero external vector databases.")
    print("  All memory isolation enforced by ORE manifests.")
    print()


if __name__ == "__main__":
    main()

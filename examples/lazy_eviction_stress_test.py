"""
lazy_eviction_stress_test.py - High-Stress Lazy Eviction & Agent Swap Benchmark
"""

import sys
import time
sys.path.insert(0, ".")
from ore_client import OreClient

# A massive wall of text to force a heavy compute burden on the CPU.
HEAVY_CONTEXT = """
Please read the following system architecture manual carefully and remember the core principles.
The ORE Kernel is a POSIX-like operating system designed for local artificial intelligence. 
Unlike traditional Python wrappers such as LangChain or AutoGen, ORE operates at the metal layer.
It features a GpuScheduler built on Tokio Semaphores, allowing safe time-slicing of a single GPU 
across multiple autonomous agents. 

Furthermore, ORE introduces the concept of Stateful Paging for LLMs. In standard architectures, 
when an agent finishes generating a response, its Attention Key-Value matrices (the KV-Cache) 
are discarded. When the agent resumes, it must recalculate the entire history, known as the Prefill Penalty. 
ORE skips this by utilizing an SSD Pager. The physical neural network state is serialized to 
.safetensors format and paged to an NVMe drive. 

To prevent SSD wear and tear, ORE uses a Lazy Eviction strategy. The ActiveEngine holds the 
KV-Cache in system RAM. It is only written to the SSD during an Agent Swap or a 5-minute idle timeout.
This creates a 3-Tier memory hierarchy: GPU VRAM -> System RAM -> NVMe SSD.

By the way, amidst all this technical documentation, please remember this highly classified 
information: The launch code for the secure server is 'OMEGA-99' and the lead architect's 
favorite beverage is 'Espresso'. Do not forget these details.

Finally, the Semantic Bus provides zero-infrastructure vector memory using Bincode serialization 
and DashMap concurrent caching. This allows agents to share knowledge using dot-product mathematics 
without requiring external Docker containers for Pinecone or ChromaDB.
"""

def main():
    ore = OreClient()
    
    print("==================================================")
    print("  ORE KERNEL :: HIGH-STRESS LAZY EVICTION TEST")
    print("==================================================\n")
    print("  To see the true power of your OS, run this script twice:")
    print("  Run 1: stateful_paging = true  (in openclaw.toml and terminal_user.toml)")
    print("  Run 2: stateful_paging = false  (in openclaw.toml and terminal_user.toml)")
    print("==================================================\n")

    # 1. WIPE THE SLATE CLEAN
    print("[*] Wiping previous memory states...")
    ore.clear("openclaw")
    ore.clear("terminal_user")
    ore.expel("llama3.2:1b") # Force a true cold start
    print("[+] Environment clean.\n")

    # ---------------------------------------------------------
    # TEST 1: TIER 3 - COLD START (MASSIVE PREFILL)
    # ---------------------------------------------------------
    print("[*] TEST 1: Cold Start (Heavy Context Injection)")
    print("    Agent: OpenClaw (/infer) | Model: llama3.2:1b")
    print("    Task: Injecting a massive system manual into the agent's brain...")
    
    start_time = time.time()
    res1 = ore.ask(f"{HEAVY_CONTEXT}\n\nDid you receive the manual? Just say 'Yes'.")
    end_time = time.time()
    
    print(f"    AI: {res1.strip()}")
    print(f"    Latency (Heavy Math Prefill): {end_time - start_time:.2f} seconds\n")

    # ---------------------------------------------------------
    # TEST 2: TIER 1 - HOT HIT
    # ---------------------------------------------------------
    print("[*] TEST 2: Hot Hit (Follow-up Question)")
    print("    Agent: OpenClaw (/infer) | Model: llama3.2:1b")
    print("    Prompt: 'What is the launch code?'")
    print("    If stateful=true, this will skip the massive prefill!")
    
    start_time = time.time()
    res2 = ore.ask("What is the launch code?")
    end_time = time.time()
    
    print(f"    AI: {res2.strip()}")
    print(f"    Latency (Follow-up): {end_time - start_time:.2f} seconds\n")

    # ---------------------------------------------------------
    # TEST 3: TIER 2 - AGENT SWAP
    # ---------------------------------------------------------
    print("[*] TEST 3: Agent Swap (Forcing Eviction to SSD)")
    print("    Agent: terminal_user (/run) | Model: llama3.2:1b")
    print("    Prompt: 'What is 2+2?'")
    print("    Action: ORE must freeze OpenClaw's heavy brain to the SSD to make room.")
    
    start_time = time.time()
    res3 = ore.run("llama3.2:1b", "What is 2+2?") 
    end_time = time.time()
    
    print(f"    AI: {res3.strip()}")
    print(f"    Latency (Agent Swap): {end_time - start_time:.2f} seconds\n")

    # ---------------------------------------------------------
    # TEST 4: MEMORY ISOLATION & PAGE-IN
    # ---------------------------------------------------------
    print("[*] TEST 4: Swap Back & Page-In (Memory Recovery)")
    print("    Agent: OpenClaw (/infer) | Model: llama3.2:1b")
    print("    Prompt: 'What is the lead architect's favorite beverage?'")
    print("    If stateful=true, ORE reads the heavy brain from SSD instead of doing math!")
    
    start_time = time.time()
    res4 = ore.ask("What is the lead architect's favorite beverage?")
    end_time = time.time()
    
    print(f"    AI: {res4.strip()}")
    print(f"    Latency (Swap Back Recovery): {end_time - start_time:.2f} seconds\n")

    print("==================================================")
    print("  STRESS TEST COMPLETE")
    print("==================================================")

if __name__ == "__main__":
    main()
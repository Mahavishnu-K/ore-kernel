import sys
import time
import requests
import threading
from pathlib import Path

# Add the parent directory to sys.path to import ore_client
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from ore_client import OreClient

def get_top(ore):
    try:
        r = requests.get(f"{ore.base_url}/top", headers=ore.headers)
        r.raise_for_status()
        return r.text
    except Exception as e:
        return f"Error getting top: {e}"

def run_agent(ore, app_id, model, prompt, thread_name=""):
    print(f"[{thread_name}] Starting agent '{app_id}' with model '{model}'...")
    try:
        # Assuming ore.run is synchronous
        res = ore.run(model, prompt, app_id=app_id)
        print(f"[{thread_name}] Agent '{app_id}' finished.")
    except Exception as e:
        print(f"[{thread_name}] Agent '{app_id}' failed: {e}")

def main():
    ore = OreClient()
    
    print("==================================================")
    print("  ORE KERNEL :: ARCHITECTURAL FIXES TEST SUITE")
    print("==================================================\n")
    
    print("[*] Wiping previous memory states to start fresh...")
    ore.clear("agent_alpha")
    ore.clear("agent_beta")
    ore.expel("llama3.2:1b")
    ore.expel("qwen2.5:0.5b")
    
    print("\n[*] Initial Scheduler Status:")
    print(get_top(ore))
    print("\n")

    # TEST 1: COLD START (Tier 3)
    print(">>> TEST 1: COLD START (agent_alpha, Llama)")
    run_agent(ore, "agent_alpha", "llama3.2:1b", "Say hello.", "T1")
    
    # TEST 2: PERFECT HIT (Tier 1)
    print("\n>>> TEST 2: PERFECT HIT (agent_alpha, Llama, same agent & model)")
    run_agent(ore, "agent_alpha", "llama3.2:1b", "Say hello again.", "T2")

    # TEST 3: AGENT SWAP (Tier 2)
    print("\n>>> TEST 3: AGENT SWAP (agent_beta, Llama, different agent, same model)")
    run_agent(ore, "agent_beta", "llama3.2:1b", "Hello from Agent B.", "T3")

    # TEST 4: EVICTION & CONCURRENCY (Multi-Model pressure)
    print("\n>>> TEST 4: EVICTION & CONCURRENCY (Spawning multiple models)")
    
    # We spawn multiple requests to put pressure on the VRAM Accountant
    threads = []
    t4 = threading.Thread(target=run_agent, args=(ore, "agent_alpha", "qwen2.5:0.5b", "Write a short poem.", "T4"))
    t5 = threading.Thread(target=run_agent, args=(ore, "agent_beta", "llama3.2:1b", "Write a haiku.", "T5"))
    t6 = threading.Thread(target=run_agent, args=(ore, "terminal_user", "qwen2.5:0.5b", "Another poem.", "T6"))
    
    threads.extend([t4, t5, t6])
    
    for t in threads:
        t.start()
    
    time.sleep(2)
    print("\n[*] Scheduler Status During High Load:")
    print(get_top(ore))
    
    for t in threads:
        t.join()
        
    print("\n[*] Final Scheduler Status:")
    print(get_top(ore))

    print("\n==================================================")
    print("  TEST COMPLETE. Check the ORE Server console logs")
    print("  to verify 'Cold Start', 'Perfect Hit', 'Agent Swap',")
    print("  and 'Exact KV-Cache Requirement' prints.")
    print("==================================================")

if __name__ == "__main__":
    main()

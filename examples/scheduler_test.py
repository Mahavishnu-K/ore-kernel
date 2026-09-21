"""
scheduler_test.py - Multi-Tenant GPU Scheduler & Bin-Packing Test
"""

import sys
import time
import requests
import threading

sys.path.insert(0, ".")
from ore_client import OreClient

def get_top(ore):
    try:
        r = requests.get(f"{ore.base_url}/top", headers=ore.headers)
        r.raise_for_status()
        return r.text
    except Exception as e:
        return f"Error getting top: {e}"

def run_agent(ore, app_id, model, prompt):
    print(f"[*] Starting agent '{app_id}' with model {model}...")
    try:
        res = ore.run(model, prompt, app_id=app_id)
        print(f"[+] Agent '{app_id}' finished.")
    except Exception as e:
        print(f"[!] Agent '{app_id}' failed: {e}")

def main():
    ore = OreClient()
    
    print("==================================================")
    print("  ORE KERNEL :: MULTI-TENANT GPU SCHEDULER TEST")
    print("==================================================\n")
    
    # 1. WIPE THE SLATE CLEAN
    print("[*] Wiping previous memory states...")
    ore.clear("openclaw")
    ore.clear("terminal_user")
    ore.clear("agent_alpha")
    ore.clear("agent_beta")
    ore.expel("llama3.2:1b")
    ore.expel("qwen2.5:0.5b")
    
    print("\n[*] Initial Scheduler Status:")
    print(get_top(ore))
    
    print("\n[*] Launching multiple agents concurrently...")
    
    # We will spawn 3 threads asking for llama3.2:1b and 1 thread asking for qwen2.5:0.5b
    # This should show multi-tenant (active_requests for llama) and bin-packing (loading multiple models).
    
    threads = []
    
    t1 = threading.Thread(target=run_agent, args=(ore, "openclaw", "llama3.2:1b", "Write a long essay about the history of Rome. Be extremely detailed."))
    t2 = threading.Thread(target=run_agent, args=(ore, "terminal_user", "llama3.2:1b", "Explain quantum physics in detail."))
    t3 = threading.Thread(target=run_agent, args=(ore, "agent_alpha", "llama3.2:1b", "Write a python script for a web server."))
    
    # qwen2.5:0.5b must be added to allowed_models in a manifest if it's checked, but let's assume we can run it.
    # We will just use llama for all to be safe, but let's try qwen2.5:0.5b.
    t4 = threading.Thread(target=run_agent, args=(ore, "agent_beta", "qwen2.5:0.5b", "Write a story about a dragon."))
    
    threads.extend([t1, t2, t3, t4])
    
    for t in threads:
        t.start()
    
    # Check scheduler status while they are running
    time.sleep(2)
    print("\n[*] Scheduler Status During Execution:")
    print(get_top(ore))
    
    for t in threads:
        t.join()
    
    print("\n[*] Scheduler Status After Execution:")
    print(get_top(ore))
    
    print("==================================================")
    print("  TEST COMPLETE")
    print("==================================================")

if __name__ == "__main__":
    main()

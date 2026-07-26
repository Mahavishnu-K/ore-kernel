import json
import time
import sys
sys.path.insert(0, "..")
from ore_client import OreClient

def main():
    ore = OreClient()
    print("==================================================")
    print("  ORE OMNI-LANGUAGE AGENT SWARM")
    print("  Orchestrating Rust, Go, JS and TS...")
    print("==================================================")

    # ---------------------------------------------------------
    # TASK 1: THE RUST MUSCLE (Markdown Compilation)
    # ---------------------------------------------------------
    print("\n[*] Agent Thought: 'I need to convert this markdown report into strict HTML.'")
    print("[*] Routing to Rust/Cargo Cartridge (rust.wasm)...")
    
    md_payload = {"markdown": "# Q3 Earnings\n**Revenue** is up by *20%*!"}
    
    start = time.perf_counter()
    res1 = ore.execute(app_id="test_agent", tool_name="rust", input_data=json.dumps(md_payload))
    latency1 = (time.perf_counter() - start) * 1000
    
    print(f"  [+] ORE Result: {res1.strip()}")
    print(f"  [+] Latency: {latency1:.2f} ms")

    # ---------------------------------------------------------
    # TASK 2: THE TYPESCRIPT VALIDATOR (Zod via NPM)
    # ---------------------------------------------------------
    print("\n[*] Agent Thought: 'I hallucinated some user data. I need to validate it against the strict Zod schema before saving.'")
    print("[*] Routing to TypeScript/NPM Cartridge (typescript.wasm)...")
    
    # Intentionally valid data (salary > 30000)
    ts_payload = {"name": "Alice Corp", "department": "Finance", "salary": 85000}
    
    start = time.perf_counter()
    res2 = ore.execute(app_id="test_agent", tool_name="typescript", input_data=json.dumps(ts_payload))
    latency2 = (time.perf_counter() - start) * 1000
    
    print(f"  [+] ORE Result: {res2.strip()}")
    print(f"  [+] Latency: {latency2:.2f} ms")

    # ---------------------------------------------------------
    # TASK 3: THE JAVASCRIPT GROUPER (Lodash via NPM)
    # ---------------------------------------------------------
    print("\n[*] Agent Thought: 'I have a messy list of items. I need to group them by category using Lodash.'")
    print("[*] Routing to JavaScript/NPM Cartridge (javascript.wasm)...")
    
    js_payload = {
        "items": [
            {"name": "Laptop", "category": "Tech"},
            {"name": "Desk", "category": "Office"},
            {"name": "Monitor", "category": "Tech"}
        ]
    }
    
    start = time.perf_counter()
    res4 = ore.execute(app_id="test_agent", tool_name="javascript", input_data=json.dumps(js_payload))
    latency3 = (time.perf_counter() - start) * 1000
    
    print(f"  [+] ORE Result:\n{res4.strip()}")
    print(f"  [+] Latency: {latency3:.2f} ms")

    # ---------------------------------------------------------
    # TASK 4: THE GO EXTRACTOR (GJSON via Go Modules)
    # ---------------------------------------------------------
    print("\n[*] Agent Thought: 'I have a massive JSON dump from a database. I need a high-speed parser to extract just the user names.'")
    print("[*] Routing to Go Modules Cartridge (go.wasm)...")
    
    go_payload = """
    {
        "company": "TechCorp",
        "users": [
            {"id": 1, "name": "Sarah"},
            {"id": 2, "name": "John"},
            {"id": 3, "name": "Michael"}
        ]
    }
    """
    
    start = time.perf_counter()
    res5 = ore.execute(app_id="test_agent", tool_name="go", input_data=go_payload)
    latency4 = (time.perf_counter() - start) * 1000
    
    print(f"  [+] ORE Result: {res5.strip()}")
    print(f"  [+] Latency: {latency4:.2f} ms")

    print("\n==================================================")
    print("  SWARM EXECUTION COMPLETE")
    print(f"  Total Orchestration Time: {latency1 + latency2 + latency3 + latency4:.2f} ms")
    print("==================================================")

if __name__ == "__main__":
    main()
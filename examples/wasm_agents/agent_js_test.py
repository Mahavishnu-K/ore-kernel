import json
import time
import sys
sys.path.insert(0, "..")
from ore_client import OreClient

def main():
    ore = OreClient()
    print("==================================================")
    print("  ORE AGENT: JAVASCRIPT TOOL CALLER")
    print("==================================================\n")
    
    print("[*] Agent Thought: 'I have raw user data. I will use the js_formatter tool to make it beautiful.'")
    
    # The raw data the AI wants to format
    payload = {
        "items": [
            {"id": 1, "name": "Alice Corp", "status": "Active", "revenue": "$45,000"},
            {"id": 2, "name": "Bob LLC", "status": "Pending", "revenue": "$12,500"},
            {"id": 3, "name": "Charlie Inc", "status": "Offline", "revenue": "$0"}
        ]
    }
    
    print("[*] Piping JSON payload into JavaScript WASM Cartridge...\n")
    
    start_time = time.perf_counter()
    
    # Execute the Tool via the ORE Kernel!
    response = ore.execute(
        app_id="wasm_agent",
        tool_name="js_formatter",
        input_data=json.dumps(payload)
    )
    
    latency = (time.perf_counter() - start_time) * 1000

    print("[+] ORE Kernel Response:")
    print(response.strip())
    print("\n" + "-" * 50)
    print(f"[+] Total Round-Trip Latency: {latency:.2f} ms")
    print("-" * 50)

if __name__ == "__main__":
    main()
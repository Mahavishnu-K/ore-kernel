import time
import json
import sys
sys.path.insert(0, "..")
from ore_client import OreClient

def main():
    ore = OreClient()
    print("[*] UI Engineer Agent booting...")
    
    # We pass a proper JSON payload now!
    payload = {
        "name": "DashboardWidget",
        "count": "5000" # Let's stress test the JS engine!
    }
    
    print(f"[*] Calling TypeScript Cartridge (ts_generator.wasm) via STDIN...")

    start_time = time.perf_counter()
    
    response = ore.execute(
        app_id="wasm_agent",
        tool_name="ts_generator",
        input_data=json.dumps(payload) # <--- Changed to input_data!
    )

    end_time = time.perf_counter()

    print("\n[+] Generated React Code from WASM:")
    print(response.strip())
    print("-" * 40)
    print(f"Total ORE Round-Trip Latency: {(end_time - start_time) * 1000:.2f} ms")

if __name__ == "__main__":
    main()
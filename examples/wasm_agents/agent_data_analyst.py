import json
import time
import sys
sys.path.insert(0, "..")
from ore_client import OreClient

def main():
    ore = OreClient()
    print("[*] Data Analyst Agent booting...")
    print("[*] Generating massive dataset to process...")
    
    # The Agent generates a massive JSON string
    massive_dataset = json.dumps({
        "description": "This is a massive JSON payload testing the STDIN pipe capabilities of the WebAssembly Sandbox.",
        "data": ["apple", "elephant", "igloo", "octopus", "umbrella"] * 1000
    })

    start_time = time.perf_counter()

    print(f"[*] Calling Rust Cartridge (rust_cruncher.wasm) via ORE STDIN Pipe...")
    
    # The Agent Framework delegates execution to the ORE Kernel
    response = ore.execute(
        app_id="wasm_agent",
        tool_name="rust_cruncher",
        input_data=massive_dataset  # Piped directly to STDIN!
    )

    end_time = time.perf_counter()

    print("\n[+] Kernel Response:")
    print(response.strip())
    print("-" * 40)
    print(f"Total ORE Round-Trip Latency: {(end_time - start_time) * 1000:.2f} ms")
    print("(Includes HTTP transit, 50µs Sandbox Boot, Rust Compute, and Sandbox Teardown)")

if __name__ == "__main__":
    main()
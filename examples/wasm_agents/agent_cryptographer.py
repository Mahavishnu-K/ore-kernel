import time
import sys
sys.path.insert(0, "..")
from ore_client import OreClient

def main():
    ore = OreClient()
    print("[*] Cryptographer Agent booting...")
    
    # The AI agent has a large document it needs to securely hash
    secret_data = "This is a highly classified document that needs to be hashed securely." * 1000
    
    print("[*] Calling Go Cartridge (go_hasher.wasm) via ORE STDIN Pipe...")

    start_time = time.perf_counter()
    
    # The Agent Framework delegates execution to the ORE Kernel
    response = ore.execute(
        app_id="wasm_agent",
        tool_name="go_hasher",
        input_data=secret_data  # Piped directly to STDIN of the Go cartridge!
    )

    end_time = time.perf_counter()

    print("\n[+] Kernel Response:")
    print(response.strip())
    print("-" * 40)
    print(f"Total ORE Round-Trip Latency: {(end_time - start_time) * 1000:.2f} ms")

if __name__ == "__main__":
    main()

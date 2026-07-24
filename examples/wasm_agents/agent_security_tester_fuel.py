import time
import sys
sys.path.insert(0, "..")
from ore_client import OreClient

def main():
    ore = OreClient()
    print("[*] Security Tester (Fuel) Agent booting...")
    print("[*] Calling Rust Cartridge (infinite_loop_trap.wasm) to verify CPU fuel limits...")

    start_time = time.perf_counter()
    
    response = ore.execute(
        app_id="wasm_agent",
        tool_name="infinite_loop_trap"
    )

    end_time = time.perf_counter()

    print("\n[+] Kernel Response (Should show Fuel Exhaustion Trap):")
    print(response.strip())
    print("-" * 40)
    print(f"Total ORE Round-Trip Latency: {(end_time - start_time) * 1000:.2f} ms")

if __name__ == "__main__":
    main()

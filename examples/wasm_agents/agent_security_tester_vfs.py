import time
import sys
sys.path.insert(0, "..")
from ore_client import OreClient

def main():
    ore = OreClient()
    print("[*] Security Tester (VFS) Agent booting...")
    print("[*] Calling Python Cartridge (vfs_security_check.wasm) to verify file system boundaries...")

    start_time = time.perf_counter()
    
    response = ore.execute(
        app_id="wasm_agent",
        tool_name="vfs_security_check"
    )

    end_time = time.perf_counter()

    print("\n[+] Kernel Response:")
    print(response.strip())
    print("-" * 40)
    print(f"Total ORE Round-Trip Latency: {(end_time - start_time) * 1000:.2f} ms")

if __name__ == "__main__":
    main()

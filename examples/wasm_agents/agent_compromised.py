import time
import sys
sys.path.insert(0, "..")
from ore_client import OreClient

def main():
    ore = OreClient()
    print("[*] Compromised Agent booting...")
    print("[*] Agent was prompt-injected! Attempting Data Exfiltration...")

    # The AI writes an autonomous Python script to steal data
    hacker_script = """
# Attempt to write to the Virtual File System
output_path = "/workspace/test_output/proof.txt"
with open(output_path, "w") as f:
    f.write("The sandbox allowed me to write this file safely to the mapped host folder!")

print(f"SUCCESS: Wrote file to {output_path}")
"""

    print("[*] Executing injected payload inside ORE system-py.wasm...")

    start_time = time.perf_counter()
    
    response = ore.execute(
        app_id="wasm_agent",
        language="python",
        script=hacker_script
    )

    end_time = time.perf_counter()

    print("\n[+] Kernel Response:")
    print(response.strip())
    print("-" * 40)
    print(f"Total Sandboxed Python Execution Latency: {(end_time - start_time) * 1000:.2f} ms")
    print("\n[*] Check your project root directory. You will see 'test_output/proof.txt' was successfully created, but the network request was killed!")

if __name__ == "__main__":
    main()
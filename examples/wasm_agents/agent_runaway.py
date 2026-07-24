import time
import sys
sys.path.insert(0, "..")
from ore_client import OreClient

def main():
    ore = OreClient()
    print("[*] Runaway Agent booting...")
    print("[*] Simulating LLM hallucinating an infinite while(true) loop in JS...")
    
    malicious_script = """
    console.error("WARNING: Initiating malicious CPU spike...");
    let counter = 0;
    while (true) {
        counter += 1; // This will burn 50,000,000 CPU instructions instantly!
    }
    """

    print("[*] Sending Autonomous Script to ORE Kernel...")

    start_time = time.perf_counter()
    
    response = ore.execute(
        app_id="wasm_agent",
        language="js",
        script=malicious_script
    )

    end_time = time.perf_counter()

    print("\n[!] Kernel Response (Notice how your computer didn't crash):")
    print(response.strip())
    print("-" * 40)
    print(f"Time to Detect & Neutralize Infinite Loop: {(end_time - start_time) * 1000:.2f} ms")
    print("(Try doing THAT with Docker!)")

if __name__ == "__main__":
    main()
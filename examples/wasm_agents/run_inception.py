import sys
import textwrap
sys.path.insert(0, "..")
from ore_client import OreClient

def main():
    ore = OreClient()
    print("==================================================")
    print("  ORE KERNEL :: INCEPTION MODE (AUTONOMOUS PYTHON)")
    print("==================================================\n")
    
    print("[*] Simulating an AI Agent writing a dynamic Python script...")
    
    # This is the exact code an AI might generate to solve a math problem
    # Notice we can use the Python Standard Library (math, sys, json) flawlessly!
    hallucinated_script = textwrap.dedent("""
        import math
        import sys
        import json

        # Let's prove we are inside the WASM Matrix
        print("--- INSIDE THE WASM SANDBOX ---")
        print(f"Python Version : {sys.version.split()[0]}")
        print(f"Platform       : {sys.platform}")
        
        # Do some math
        data = {"target": 256, "multiplier": 3.14159}
        result = math.sqrt(data["target"]) * data["multiplier"]
        
        print(f"Calculation    : {result}")
        print("INCEPTION MODE ACTIVATED.")
    """).strip()

    print("\n[+] The AI wrote this script:")
    print("--------------------------------------------------")
    print(hallucinated_script)
    print("--------------------------------------------------\n")

    print("[*] Sending to ORE Kernel for Zero-Trust Execution...\n")

    try:
        # We use the 'wasm_agent' manifest because it allows 'python' execution
        response = ore.execute(
            app_id="wasm_agent",      
            language="python",
            script=hallucinated_script
        )
        
        print("[+] ORE Sandbox Output:")
        # Indent the output slightly to make it look clean
        for line in response.strip().split('\n'):
            print(f"    {line}")
            
    except Exception as e:
        print(f"[-] Execution Failed: {e}")
        
    print("\n==================================================")

if __name__ == "__main__":
    main()
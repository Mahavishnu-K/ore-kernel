import sys
import time
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
    ai_generated_script = textwrap.dedent("""
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
    print(ai_generated_script)
    print("--------------------------------------------------\n")

    print("[*] Sending to ORE Kernel for Zero-Trust Execution...\n")

    try:
        # We use the 'wasm_agent' manifest because it allows 'python' execution

        start_time = time.perf_counter()

        response = ore.execute(
            app_id="wasm_agent",      
            language="python",
            script=ai_generated_script
        )

        end_time = time.perf_counter()
        print(f"Total ORE Round-Trip Latency: {(end_time - start_time) * 1000:.2f} ms")
        
        print("\n[+] ORE Sandbox Output:")
        # Indent the output slightly to make it look clean
        for line in response.strip().split('\n'):
            print(f"    {line}")
            
    except Exception as e:
        print(f"[-] Execution Failed: {e}")
        
    print("\n==================================================")
    print("  ORE INCEPTION MODE: JAVASCRIPT / QUICKJS")
    print("==================================================\n")
    
    print("[*] AI Agent is writing a dynamic JavaScript payload...\n")
    
    # The AI hallucinates this script on the fly
    ai_generated_js = """
    console.log("Hello from WasmEdge QuickJS inside the ORE OS!");
    
    // Let's do some math
    const numbers = [10, 20, 30, 40, 42];
    const sum = numbers.reduce((a, b) => a + b, 0);
    
    console.log(`The calculated sum is: ${sum}`);
    
    // Prove we are in the VFS
    console.log("Script executed securely from the Ephemeral VFS.");
    """
    
    print("[*] Sending payload to ORE Kernel...\n")
    
    try:
        # Execute the Script via the ORE Kernel!

        start_time = time.perf_counter()
        response = ore.execute(
            app_id="wasm_agent",      
            language="js",
            script=ai_generated_js
        )
        end_time = time.perf_counter()
        print(f"Total ORE Round-Trip Latency: {(end_time - start_time) * 1000:.2f} ms")
        
        print("[+] ORE Sandbox Output:")
        for line in response.strip().split('\n'):
            print(f"    {line}")
            
    except Exception as e:
        print(f"[-] Execution Failed: {e}")
        
    print("\n==================================================")

if __name__ == "__main__":
    main()
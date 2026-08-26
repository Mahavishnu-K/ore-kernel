# examples/run_linker.py
import sys
sys.path.insert(0, "..")
from ore_client import OreClient

def main():
    ore = OreClient()
    print("==================================================")
    print("  ORE-LD DYNAMIC LINKER TEST")
    print("==================================================\n")
    
    print("[*] Triggering 'linker_test.wasm' in the Sandbox...")
    
    # Execute the Tool via the ORE Kernel!
    response = ore.execute(
        app_id="test_agent",      # Must match the manifest where you added the tool
        tool_name="linker_test"   # Matches the ~/.ore/tools/linker_test.wasm file
    )
    
    print("\n[+] ORE Sandbox Output:\n")
    print(response.strip())
    print("\n==================================================")

if __name__ == "__main__":
    main()
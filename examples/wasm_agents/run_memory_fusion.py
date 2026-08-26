# examples/run_memory_fusion.py
import sys
sys.path.insert(0, "..")
from ore_client import OreClient

def main():
    ore = OreClient()
    print("==================================================")
    print("  ORE-LD MULTI-LANGUAGE MEMORY FUSION TEST")
    print("==================================================\n")
    
    tools = [
        ("Rust (Cybersecurity)", "crypto_tool"),
        ("C (FinTech)", "finance_tool"),
        ("C++ (Data Engineering)", "data_tool"),
        ("Zig (IoT/Edge)", "iot_tool")
    ]
    
    for desc, tool in tools:
        print(f"[*] Triggering {desc} Host Agent ('{tool}.wasm')...")
        
        try:
            # Execute the Tool via the ORE Kernel!
            response = ore.execute(
                app_id="test_agent",      
                tool_name=tool   
            )
            
            print("[+] ORE Sandbox Output:")
            # We indent the output slightly to make it look clean
            for line in response.strip().split('\n'):
                print(f"    {line}")
                
        except Exception as e:
            print(f"[-] Execution Failed: {e}")
            
        print("\n" + "="*50 + "\n")

if __name__ == "__main__":
    main()
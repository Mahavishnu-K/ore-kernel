import sys
sys.path.insert(0, "..")
from ore_client import OreClient

def main():
    ore = OreClient()
    print("==================================================")
    print("  ORE-LD CROSS-LANGUAGE MEMORY FUSION TEST")
    print("==================================================\n")
    
    tools = [
        ("C Host -> Rust Plugin", "c_uses_rust"),
        ("Rust Host -> C++ Plugin", "rust_uses_cpp"),
        ("Zig Host -> C Plugin", "zig_uses_c")
    ]
    
    for desc, tool in tools:
        print(f"[*] Triggering {desc} ('{tool}.wasm')...")
        
        try:
            response = ore.execute(
                app_id="test_agent",      
                tool_name=tool   
            )
            
            print("[+] ORE Sandbox Output:")
            for line in response.strip().split('\n'):
                print(f"    {line}")
                
        except Exception as e:
            print(f"[-] Execution Failed: {e}")
            
        print("\n" + "="*50 + "\n")

if __name__ == "__main__":
    main()
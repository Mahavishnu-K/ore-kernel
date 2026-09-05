import sys
import json
sys.path.insert(0, "..")
from ore_client import OreClient

def main():
    ore = OreClient()
    print("==================================================")
    print("  ORE KERNEL :: VFS DIRECTORY TEST")
    print("==================================================\n")
    
    print("[*] Triggering python project tool...")
    
    # Send dynamic STDIN data to the tool
    payload = {
        "user": "Alice",
        "score": 42
    }

    try:
        response = ore.execute(
            app_id="test_agent",      
            tool_name="python",
            input_data=json.dumps(payload)
        )
        
        print("\n[+] ORE Sandbox Output:\n")
        for line in response.strip().split('\n'):
            print(f"    {line}")
            
    except Exception as e:
        print(f"[-] Execution Failed: {e}")

    print("\n")
    print("==================================================")
    print("  ORE KERNEL :: SINGLE-FILE PYTHON VFS TEST")
    print("==================================================\n")
    
    print("[*] Triggering 'simple_script'...")
    
    payload = {
        "task": "Analyze security protocols"
    }

    try:
        response = ore.execute(
            app_id="test_agent",      
            tool_name="simple_script",
            input_data=json.dumps(payload)
        )
        
        print("\n[+] ORE Sandbox Output:\n")
        for line in response.strip().split('\n'):
            print(f"    {line}")
            
    except Exception as e:
        print(f"[-] Execution Failed: {e}")
        
    print("\n==================================================")
        
if __name__ == "__main__":
    main()
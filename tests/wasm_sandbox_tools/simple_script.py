# tests/wasm_sandbox_tools/simple_script.py
import sys
import json

print("--- SINGLE FILE PYTHON TOOL ACTIVATED ---")
print(f"Engine: Python {sys.version.split()[0]} on {sys.platform}")

# Read dynamic JSON data passed from the ORE Kernel
raw_input = sys.stdin.read()

if not raw_input:
    print("No STDIN payload provided by Kernel.")
    sys.exit(1)

try:
    data = json.loads(raw_input)
    task = data.get("task", "Unknown")
    
    print(f"\n[Processing Task] -> {task}")
    
    # Do some quick logic
    result = {
        "status": "success",
        "task_length": len(task),
        "reversed": task[::-1]
    }
    
    print(f"\nResult: {json.dumps(result)}")
    
except Exception as e:
    print(f"Fatal Error: {e}")
import sys
import json
from colorama import Fore, Style, init

# Import from our local multi-folder structure!
from core.engine import load_settings, process_template

init(autoreset=True, strip=False)

print(Fore.CYAN + "=== VFS PYTHON AGENT ===" + Style.RESET_ALL)

# 1. Test File System Read (JSON)
try:
    settings = load_settings()
    print(f"Loaded Settings: {settings['agent_name']} v{settings['version']}")
except Exception as e:
    print(Fore.RED + f"Failed to load settings: {e}" + Style.RESET_ALL)
    sys.exit(1)

# 2. Test STDIN Read from the ORE Kernel
raw_input = sys.stdin.read()
if not raw_input:
    print(Fore.RED + "No STDIN payload provided by Kernel." + Style.RESET_ALL)
    sys.exit(1)

try:
    payload = json.loads(raw_input)
    user = payload.get("user", "Unknown")
    
    # 3. Do some math combining the JSON file and the STDIN payload
    base_score = payload.get("score", 0)
    final_score = base_score * settings["max_retries"]
    
    # 4. Test File System Read (Text) + Module Logic
    result_text = process_template(user, final_score)
    
    print(Fore.GREEN + "\n[SUCCESS] " + Style.RESET_ALL + result_text)
except Exception as e:
    print(Fore.RED + f"Processing Error: {e}" + Style.RESET_ALL)
"""
run_plugin_lifecycle.py - ore-ld plugin lifecycle test (ore_dlclose / ore_dlerror / slab recycling)

Runs the six lifecycle host tools from tests/memory_fusion and tests/cross_lang_memory_fusion
through the ORE sandbox and checks their PASS/FAIL output.

Build the plugins and hosts first (see the READMEs in those two folders), then restart the
kernel so it picks up the updated manifests/test_agent.toml.
"""

import sys
sys.path.insert(0, "..")
from ore_client import OreClient

APP_ID = "test_agent"

# (description, tool name, expected outcome)
#   "pass" -> the host must print "ALL CHECKS PASSED"
#   "trap" -> the host must end in a WebAssembly trap (stale function pointer after dlclose)
TESTS = [
    ("C Host    -> C Plugin    : dlclose, dlerror, slab reuse", "lifecycle_tool", "pass"),
    ("Rust Host -> Rust Plugins: hot-swap v1 -> v2",            "hotswap_tool", "pass"),
    ("Zig Host  -> Zig Plugin  : stale pointer must trap",      "stale_pointer_tool", "trap"),
    ("Rust Host -> C Plugin    : load/drop 5x, one slab",       "rust_recycles_c", "pass"),
    ("C++ Host  -> Rust Plugins: cross-language hot-swap",      "cpp_hotswaps_rust", "pass"),
    ("Zig Host  -> C++ Plugin  : error reporting",              "zig_errors_from_cpp", "pass"),
]


def diagnose(output: str) -> str | None:
    """Turns common setup mistakes into a hint."""
    if "not whitelisted" in output:
        return "Tool not in manifests/test_agent.toml allowed_tools (restart the kernel after editing it)."
    if "Failed to read WASM binary" in output:
        return "Host not built. Run `ore mktool <file> --host`."
    if "not found in ~/.ore/plugins/" in output and "PASS" not in output:
        return "Plugin not built. Run `ore mktool <file> --shared`."
    return None


def judge(output: str, expected: str) -> tuple[bool, str]:
    failed_checks = [line.strip() for line in output.splitlines() if " FAIL " in f" {line} "]

    if expected == "pass":
        if failed_checks:
            return False, f"{len(failed_checks)} check(s) failed"
        if "KERNEL ERROR" in output:
            return False, "unexpected trap or kernel error"
        if "ALL CHECKS PASSED" not in output:
            return False, "host did not finish"
        return True, "all checks passed"

    # expected == "trap"
    if "stale pointer still ran" in output:
        return False, "stale pointer was NOT invalidated"
    if failed_checks:
        return False, f"{len(failed_checks)} check(s) failed before the trap"
    if "PASS close() succeeded" not in output:
        return False, "host did not reach the close step"
    if "KERNEL ERROR" not in output:
        return False, "expected a trap, but the host exited normally"
    return True, "stale pointer trapped as expected"


def kernel_alive(ore: OreClient) -> bool:
    try:
        ore.health()
        return True
    except Exception:
        return False


def main():
    ore = OreClient()
    print("==================================================")
    print("  ORE-LD PLUGIN LIFECYCLE TEST")
    print("  ore_dlclose · ore_dlerror · slab recycling")
    print("==================================================\n")

    results = []

    for desc, tool, expected in TESTS:
        print(f"[*] {desc}  ('{tool}.wasm')")

        try:
            output = ore.execute(app_id=APP_ID, tool_name=tool)
        except Exception as e:
            output = f"KERNEL ERROR: request failed: {e}"

        print("[+] ORE Sandbox Output:")
        for line in output.strip().split("\n"):
            print(f"    {line}")

        ok, reason = judge(output, expected)
        hint = None if ok else diagnose(output)
        print(f"\n[{'PASS' if ok else 'FAIL'}] {tool}: {reason}")
        if hint:
            print(f"       hint: {hint}")

        # A trapping plugin must take down only its own execution, never the kernel.
        if expected == "trap":
            alive = kernel_alive(ore)
            print(f"[{'PASS' if alive else 'FAIL'}] kernel still online after the trap")
            if not alive:
                ok, reason = False, "kernel went down after the trap"

        results.append((tool, ok, reason))
        print("\n" + "=" * 50 + "\n")

    # Summary
    passed = sum(1 for _, ok, _ in results if ok)
    print("SUMMARY")
    for tool, ok, reason in results:
        print(f"  [{'PASS' if ok else 'FAIL'}] {tool:<22} {reason}")
    print(f"\n  {passed}/{len(results)} passed")

    sys.exit(0 if passed == len(results) else 1)


if __name__ == "__main__":
    main()

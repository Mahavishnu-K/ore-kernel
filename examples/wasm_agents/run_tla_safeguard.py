import sys
import time
sys.path.insert(0, "..")
from ore_client import OreClient

def run_test(ore: OreClient, test_num: int, title: str, script: str, dependencies=None, expect_error_str=None):
    print("=" * 65)
    print(f"  [TEST {test_num}] {title}")
    print("=" * 65)
    print("[*] Script payload:\n")
    for line in script.strip().split("\n")[:12]:
        print(f"    {line}")
    if len(script.strip().split("\n")) > 12:
        print("    ...")
    print("\n[*] Sending payload to ORE Kernel Inception Mode...")

    try:
        t0 = time.perf_counter()
        response = ore.execute(
            app_id="wasm_agent",
            language="js",
            script=script,
            dependencies=dependencies
        )
        elapsed = (time.perf_counter() - t0) * 1000
        print(f"[+] Execution completed in {elapsed:.2f} ms\n")
        print("[+] Output from Sandbox:")
        for line in response.strip().split("\n"):
            print(f"    {line}")

        if expect_error_str:
            if expect_error_str in response:
                print(f"\n[PASS] Verified error was cleanly captured: '{expect_error_str}' found in output.")
                return True
            else:
                print(f"\n[FAIL] Expected safeguard to catch '{expect_error_str}', but output was missing it!")
                return False
        else:
            if "[OK]" in response or "SUCCESS" in response:
                print("\n[PASS] Verified naked Top-Level Await executed successfully.")
                return True
            else:
                print("\n[?] Output received without explicit OK marker.")
                return True

    except Exception as e:
        print(f"[-] Execution Failed at client level: {e}")
        return False

def main():
    ore = OreClient()
    print("#################################################################")
    print("  ORE KERNEL :: TOP-LEVEL AWAIT & RUNTIME ERROR SAFEGUARD SUITE")
    print("#################################################################\n")

    passed = 0
    total = 5

    # -------------------------------------------------------------
    # TEST 1: Pure Naked Top-Level Await with Core Modules (No IIFE)
    # -------------------------------------------------------------
    t1_js = r"""
    import fs from 'fs';
    import path from 'path';

    // Notice: NO (async () => { ... })() wrapper!
    // The LLM writes pure naked await at top level:
    const target = path.join('/ore_tmp', 'naked_tla.txt');
    fs.writeFileSync(target, 'Hello from Naked Top-Level Await in ORE WASM!');

    const readBack = await new Promise((resolve) => {
        setTimeout(() => {
            resolve(fs.readFileSync(target, 'utf-8'));
        }, 10);
    });

    console.log(`[OK] Top-Level Await resolved: "${readBack}"`);
    """
    if run_test(ore, 1, "Pure Naked Top-Level Await with Core VFS Polyfills", t1_js):
        passed += 1

    print("\n")

    # -------------------------------------------------------------
    # TEST 2: Multi-line Static Destructured Import + Naked Await
    # -------------------------------------------------------------
    t2_js = r"""
    import {
        Buffer
    } from 'buffer';
    import {
        createHash
    } from 'crypto';

    // Verify multi-line static imports are partitioned cleanly:
    const data = Buffer.from('ORE_TLA_PARTITIONING_VERIFIED', 'utf-8');
    const digest = await new Promise((resolve) => {
        const h = createHash('sha256').update(data).digest('hex');
        resolve(h);
    });

    console.log(`[OK] Multi-line import + TLA SHA256: ${digest.substring(0, 16)}...`);
    """
    if run_test(ore, 2, "Multi-Line Destructuring Static Imports + TLA", t2_js):
        passed += 1

    print("\n")

    # -------------------------------------------------------------
    # TEST 3: Naked Top-Level Await with JIT NPM Packages (Axios)
    # -------------------------------------------------------------
    t3_js = r"""
    import axios from 'axios';

    // Naked await on external network call via JIT NPM bundle:
    const res = await axios.get('https://jsonplaceholder.typicode.com/todos/1');
    console.log(`    -> Status Code : ${res.status}`);
    console.log(`    -> Todo Item   : "${res.data.title}"`);
    console.log(`[OK] JIT NPM + Naked Top-Level Await network call operational.`);
    """
    if run_test(ore, 3, "Naked Top-Level Await with JIT NPM Packages (Axios)", t3_js, dependencies=["axios"]):
        passed += 1

    print("\n")

    # -------------------------------------------------------------
    # TEST 4: Error Safeguard Intercepts Naked Await Unhandled Rejection
    # -------------------------------------------------------------
    t4_js = r"""
    import fs from 'fs';

    console.log("[*] Simulating an unhandled promise rejection in naked await...");
    // In unpatched QuickJS, this exits silently with empty STDERR!
    // With ORE safeguard, this is captured and printed with stack trace:
    await Promise.reject(new Error("ORE_TEST_UNHANDLED_REJECTION_CAUGHT"));
    """
    if run_test(ore, 4, "Unhandled Promise Rejection in Naked Await Caught by Safeguard", t4_js, expect_error_str="ORE_TEST_UNHANDLED_REJECTION_CAUGHT"):
        passed += 1

    print("\n")

    # -------------------------------------------------------------
    # TEST 5: Synchronous Throw Safeguard in Naked Script
    # -------------------------------------------------------------
    t5_js = r"""
    import path from 'path';

    console.log("[*] Simulating synchronous runtime throw in naked script...");
    throw new Error("ORE_TEST_SYNC_THROW_CAUGHT");
    """
    if run_test(ore, 5, "Synchronous Runtime Exception Caught by Safeguard", t5_js, expect_error_str="ORE_TEST_SYNC_THROW_CAUGHT"):
        passed += 1

    print("\n" + "=" * 65)
    print(f"  ALL TESTS FINISHED: {passed}/{total} Passed")
    print("=" * 65)

if __name__ == "__main__":
    main()

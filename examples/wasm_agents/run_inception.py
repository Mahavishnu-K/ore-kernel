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

    print("==================================================")
    print("  ORE KERNEL :: INCEPTION MODE (WITH PIP DEPS)")
    print("==================================================\n")
    
    print("[*] Simulating an AI Agent writing a script that needs PyPI packages...")
    
    # We ask for 'colorama' for ANSI colors, and 'cowsay' just to prove it downloads pure python packages!
    ai_dependencies = ["colorama", "cowsay"]
    
    ai_generated_script_deps = textwrap.dedent("""
        import sys
        import cowsay
        from colorama import Fore, Style, init

        # Initialize colorama
        init(autoreset=True)

        print(Fore.GREEN + "--- INSIDE THE MATRIX ---" + Style.RESET_ALL)
        print(f"Platform: {sys.platform}")
        
        # Prove the PIP package works!
        cowsay.cow("JIT Pip Vendoring is Flawless!")

        print(Fore.CYAN + "Execution completed safely via ORE Kernel." + Style.RESET_ALL)
    """).strip()

    print(f"\n[+] The AI requested dependencies: {ai_dependencies}")

    print("[*] Sending to ORE Kernel for Zero-Trust JIT Execution...\n")

    try:
        # We use the 'wasm_agent' manifest because it allows 'python'
        start_time = time.perf_counter()
        response = ore.execute(
            app_id="wasm_agent",      
            language="python",
            script=ai_generated_script_deps,
            dependencies=ai_dependencies
        )
        end_time = time.perf_counter()
        print(f"Total ORE Round-Trip Latency: {(end_time - start_time) * 1000:.2f} ms")

        print("[+] ORE Sandbox Output:\n")
        for line in response.strip().split('\n'):
            print(f"    {line}")
            
    except Exception as e:
        print(f"[-] Execution Failed: {e}")
        
    print("\n==================================================")
    print("  ORE INCEPTION MODE: JAVASCRIPT / QUICKJS")
    print("==================================================\n")
    
    print("[*] AI Agent is writing a dynamic JavaScript payload...\n")
    
    # The AI hallucinates this script on the fly
    ai_generated_js = r"""
    import assert from 'assert';
    import { Buffer } from 'buffer';
    import crypto from 'crypto';
    import EventEmitter from 'events';
    import fs from 'fs';
    import os from 'os';
    import path from 'path';
    import process from 'process';
    import querystring from 'querystring';
    import util from 'util';

    console.log("--- STARTING MODULE DIAGNOSTICS ---\\n");

    try {
        // 1. ASSERT & BUFFER
        const b = Buffer.from("ORE_OS", "utf-8");
        assert.strictEqual(b.toString('hex'), "4f52455f4f53");
        console.log("[OK] 'assert' and 'buffer' linked correctly.");

        // 2. CRYPTO
        const hash = crypto.createHash('sha256').update('ORE').digest('hex');
        console.log(`[OK] 'crypto' linked correctly (SHA256: ${hash.substring(0, 10)}...)`);

        // 3. EVENTS
        const ee = new EventEmitter();
        let fired = false;
        ee.on('ping', () => { fired = true; });
        ee.emit('ping');
        assert.strictEqual(fired, true);
        console.log("[OK] 'events' linked correctly.");

        // 4. FS & PATH (Testing the VFS Portal!)
        const testPath = path.join('/ore_tmp', 'diagnostic.txt');
        fs.writeFileSync(testPath, 'Secure VFS Check');
        const readData = fs.readFileSync(testPath, 'utf-8');
        assert.strictEqual(readData, 'Secure VFS Check');
        console.log("[OK] 'fs' and 'path' linked and VFS is operational.");

        // 5. OS & PROCESS
        console.log(`[OK] 'os' and 'process' linked (Platform: ${os.platform()}, PID: ${process.pid})`);

        // 6. QUERYSTRING
        const qs = querystring.stringify({ os: 'ore', version: 1 });
        assert.strictEqual(qs, 'os=ore&version=1');
        console.log("[OK] 'querystring' linked correctly.");

        // 7. UTIL
        const text = new util.TextEncoder().encode("Hello");
        assert.strictEqual(text.length, 5);
        console.log("[OK] 'util' linked correctly.");

        console.log("\\n--- ALL MODULES PASSED FLAWLESSLY ---");

    } catch (err) {
        console.error("\\n[FATAL ERROR]:", err.message);
        console.error(err.stack);
    }
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
    print("  ORE INCEPTION MODE: PDF GENERATION (pdf-lib)")
    print("==================================================\n")
    
    generated_pdf_js = r"""
    import { PDFDocument, rgb, StandardFonts } from 'pdf-lib';
    import fs from 'fs';
    import { Buffer } from 'buffer';

    // We use an async IIFE because pdf-lib is promise-based
    (async () => {
        console.log("--- AI PDF GENERATION INITIATED ---\n");

        try {
            console.log("[*] Creating new PDF document in WASM RAM...");
            const pdfDoc = await PDFDocument.create();
            
            // Embed a standard font
            const timesRomanFont = await pdfDoc.embedFont(StandardFonts.TimesRoman);
            
            // Add a blank page
            const page = pdfDoc.addPage([600, 400]);
            
            // Draw some text!
            page.drawText('ORE Kernel: Mission Accomplished!', {
                x: 50,
                y: 300,
                size: 30,
                font: timesRomanFont,
                color: rgb(0, 0.53, 0.71)
            });

            page.drawText('This PDF was generated autonomously inside a strict\nWebAssembly sandbox and written to the host OS.', {
                x: 50,
                y: 250,
                size: 16,
                font: timesRomanFont,
                color: rgb(0, 0, 0)
            });

            // Serialize the PDFDocument to bytes (a Uint8Array)
            console.log("[*] Serializing PDF to binary...");
            const pdfBytes = await pdfDoc.save();

            // Write the raw binary buffer directly to the Host OS!
            const targetFile = '/workspace/test_output/agent_report.pdf';
            console.log(`[*] Writing ${pdfBytes.length} bytes to ${targetFile}...`);
            
            // Your custom fs.writeFileSync handles Uint8Array natively now!
            fs.writeFileSync(targetFile, pdfBytes);
            
            console.log(`\n[OK] PDF successfully generated and saved!`);

        } catch (err) {
            console.log("\n[ERROR CAUGHT]:", err.message);
            if (err.stack) console.log(err.stack);
        }
    })();
    """

    print("\n[*] Sending PDF generation payload to ORE Kernel...\n")
    try:
        start_time = time.perf_counter()
        response = ore.execute(
            app_id="wasm_agent",      
            language="js",
            script=generated_pdf_js,
            dependencies=["pdf-lib"]  # Ensure pdf-lib is available in the WASM environment
        )
        end_time = time.perf_counter()
        print(f"Total ORE Round-Trip Latency: {(end_time - start_time) * 1000:.2f} ms")
        
        print("[+] ORE Sandbox Output:")
        for line in response.strip().split('\n'):
            print(f"    {line}")
    except Exception as e:
        print(f"[-] Execution Failed: {e}")

if __name__ == "__main__":
    main()
import sys
import time
import textwrap
sys.path.insert(0, "..")
from ore_client import OreClient

def main():
    ore = OreClient()
    print("==================================================")
    print("  ORE KERNEL :: PYTHON NETWORK PORTAL TEST")
    print("==================================================\n")
    
    print("[*] Sending Python networking payload to ORE Kernel...")
    
    # This tests the Monkey-Patched sys.modules['requests'] and 'urllib'
    python_payload = textwrap.dedent("""
        import requests
        import httpx
        import urllib.request
        import json
        import os

        print("--- PYTHON NETWORK DIAGNOSTICS ---")

        try:
            # Standard GET Request
            print("[*] Testing requests.get()...")
            res_get = requests.get('https://jsonplaceholder.typicode.com/todos/1')
            print(f"    [OK] Received: {res_get.json()['title']}")

            # Test 'httpx' (Modern Async/Sync client)
            print("[*] Testing httpx.get()...")
            r_httpx = httpx.get('https://jsonplaceholder.typicode.com/users/2')
            print(f"    [OK] HTTPX User: {r_httpx.json()['name']}")

            # Standard POST Request (Sends JSON body)
            print("[*] Testing requests.post()...")
            payload = {"title": "ORE Kernel", "body": "Zero Trust Firewall", "userId": 1}
            res_post = requests.post('https://jsonplaceholder.typicode.com/posts', json=payload)
            print(f"    [OK] Created Post ID: {res_post.json().get('id')}")

            # Standard PUT Request (Update)
            print("[*] Testing requests.put()...")
            res_put = requests.put('https://jsonplaceholder.typicode.com/posts/1', json={"id": 1, "title": "Updated"})
            print(f"    [OK] Updated Title: {res_put.json().get('title')}")

            # Standard Library Urllib (Used heavily by older PIP packages)
            print("[*] Testing urllib.request.urlopen()...")
            req = urllib.request.urlopen('https://jsonplaceholder.typicode.com/users/1')
            user_data = json.loads(req.read().decode('utf-8'))
            print(f"    [OK] Urllib User: {user_data['name']}")

            # Test Binary Download & VFS Write
            print("[*] Testing File Download to Host OS...")
            dl_res = requests.get('https://jsonplaceholder.typicode.com/posts')
            
            out_path = '/workspace/test_output/python_downloaded_posts.json'
            with open(out_path, 'wb') as f:
                f.write(dl_res.content)
            
            size = os.path.getsize(out_path)
            print(f"    [OK] Downloaded & Saved {size} bytes to {out_path}")

            print("\\n[SUCCESS] All Python network vectors operational and firewalled.")

        except Exception as e:
            print(f"\\n[FATAL ERROR]: {str(e)}")
    """).strip()

    try:
        start_time = time.perf_counter()
        response = ore.execute(
            app_id="wasm_agent",      
            language="python",
            script=python_payload,
            dependencies=["requests", "httpx"]
        )
        end_time = time.perf_counter()
        print(f"Total ORE Round-Trip Latency: {(end_time - start_time) * 1000:.2f} ms")
        
        print("\n[+] ORE Sandbox Output:")
        for line in response.strip().split('\n'):
            print(f"    {line}")
            
    except Exception as e:
        print(f"[-] Execution Failed: {e}")

    print("==================================================")
    print("  ORE KERNEL :: PYTHON ASYNCIO PARALLEL TEST      ")
    print("==================================================\n")

    normal_python_payload = textwrap.dedent("""
        import httpx
        import asyncio
        import time

        print("--- RUNNING STANDARD HTTPX ASYNC CODE ---")

        urls = [
            'https://jsonplaceholder.typicode.com/todos/1',
            'https://jsonplaceholder.typicode.com/todos/2',
            'https://jsonplaceholder.typicode.com/todos/3',
            'https://jsonplaceholder.typicode.com/todos/4',
            'https://jsonplaceholder.typicode.com/todos/5'
        ]

        async def fetch_item(client, url):
            # Standard httpx call!
            response = await client.get(url)
            return response.json()

        async def main():
            start = time.perf_counter()

            # 100% Standard idiomatic Python AsyncClient
            async with httpx.AsyncClient() as client:
                tasks = [fetch_item(client, url) for url in urls]
                results = await asyncio.gather(*tasks)

            elapsed = (time.perf_counter() - start) * 1000
            print(f"[OK] Fetched {len(results)} items in parallel in {elapsed:.2f} ms:")
            for item in results:
                print(f"    - ID {item['id']}: {item['title']}")

        asyncio.run(main())
    """).strip()

    response = ore.execute(
        app_id="wasm_agent",
        language="python",
        script=normal_python_payload
    )

    print("\n[+] ORE Sandbox Output:")
    for line in response.strip().split('\n'):
        print(f"    {line}")

    print("\n==================================================")
    print("  ORE KERNEL :: JAVASCRIPT NETWORK PORTAL TEST")
    print("==================================================\n")
    
    print("[*] Sending JavaScript networking payload to ORE Kernel...")
    
    # This tests the global fetch(), Node.js http/https streams, and NPM compatibility 
    javascript_payload = r"""
    import axios from 'axios';
    import fetch from 'node-fetch'; // Testing third-party fetch overrides!
    import http from 'http';
    import https from 'https'; // ESBuild correctly aliases this to our http.js polyfill!
    import fs from 'fs';
    import path from 'path';

    (async () => {
        console.log("--- JAVASCRIPT NETWORK DIAGNOSTICS ---\n");

        try {
            // Global Fetch API (GET)
            console.log("[*] Testing global fetch()...");
            const res1 = await fetch('https://jsonplaceholder.typicode.com/todos/2');
            const data1 = await res1.json();
            console.log(`    [OK] Fetch GET Received: ${data1.title}`);

            // Test AXIOS (Relies heavily on Node.js 'http' ClientRequest streams)
            console.log("[*] Testing axios.get()...");
            const axRes = await axios.get('https://jsonplaceholder.typicode.com/users/3');
            console.log(`    [OK] Axios User: ${axRes.data.name}`);

            // Test NODE-FETCH (Third-party fetch polyfill)
            console.log("[*] Testing node-fetch...");
            const nfRes = await fetch('https://jsonplaceholder.typicode.com/users/4');
            const nfData = await nfRes.json();
            console.log(`    [OK] Node-Fetch User: ${nfData.name}`);

            // Global Fetch API (POST)
            console.log("[*] Testing global fetch() POST...");
            const res2 = await fetch('https://jsonplaceholder.typicode.com/posts', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ title: 'WASM Sandbox', body: 'Inception', userId: 2 })
            });
            const data2 = await res2.json();
            console.log(`    [OK] Fetch POST ID: ${data2.id}`);

            // Node.js Streaming API (Used by Axios, Node-Fetch, Got)
            // This is the ultimate test of your ClientRequest and IncomingMessage classes!
            console.log("[*] Testing Node.js Streams (https.get)...");
            
            https.get('https://jsonplaceholder.typicode.com/users/2', (res) => {
                let rawData = '';
                
                // Read from the IncomingMessage stream (which reads from the VFS SSD file)
                res.on('data', (chunk) => { 
                    rawData += chunk; 
                });
                
                res.on('end', () => {
                    const parsedData = JSON.parse(rawData);
                    console.log(`    [OK] Node.js Stream User: ${parsedData.name}`);
                });

                res.on('error', (e) => {
                    console.error(`    [!] Stream Error: ${e.message}`);
                });
            });

            // Test Binary Download & VFS Write via Axios ArrayBuffer
            console.log("[*] Testing Binary File Download via Axios...");
            const dlRes = await axios.get('https://jsonplaceholder.typicode.com/comments', { 
                responseType: 'arraybuffer' 
            });
            
            const outPath = '/workspace/test_output/js_downloaded_comments.json';
            
            // Your custom fs.writeFileSync handles ArrayBuffers perfectly!
            fs.writeFileSync(outPath, dlRes.data);
            
            const stat = fs.statSync(outPath);
            console.log(`    [OK] Downloaded & Saved ${stat.size} bytes to ${outPath}`);

            console.log("\n--- TESTING PARALLEL CONCURRENT FETCHES ---");

            const urls = [
                'https://jsonplaceholder.typicode.com/todos/1',
                'https://jsonplaceholder.typicode.com/todos/2',
                'https://jsonplaceholder.typicode.com/todos/3',
                'https://jsonplaceholder.typicode.com/todos/4',
                'https://jsonplaceholder.typicode.com/todos/5'
            ];

            // Fire 5 requests SIMULTANEOUSLY via Promise.all
            console.log("[*] Dispatching 5 concurrent requests with Promise.all...");
            const start = Date.now();

            const responses = await Promise.all(urls.map(url => fetch(url).then(r => r.json())));

            const elapsed = Date.now() - start;
            console.log(`[OK] All 5 requests completed in ${elapsed} ms:`);
            responses.forEach((data, i) => {
                console.log(`    Item ${i + 1}: ${data.title}`);
            });

            console.log("\n[SUCCESS] All JS NPM network vectors operational.");
        } catch (err) {
            console.log("\n[FATAL ERROR]:", err.message);
            if (err.stack) console.log(err.stack);
        }
    })();
    """

    try:
        start_time = time.perf_counter()
        response = ore.execute(
            app_id="wasm_agent",      
            language="js",
            script=javascript_payload,
            dependencies=["axios", "node-fetch"]
        )
        end_time = time.perf_counter()
        print(f"Total ORE Round-Trip Latency: {(end_time - start_time) * 1000:.2f} ms")
        
        print("\n[+] ORE Sandbox Output:")
        for line in response.strip().split('\n'):
            print(f"    {line}")
            
    except Exception as e:
        print(f"[-] Execution Failed: {e}")
        
    print("\n==================================================")

if __name__ == "__main__":
    main()
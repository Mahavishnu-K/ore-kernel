// Tell the IDE that the Javy WASM runtime will inject this global variable
declare const Javy: any;

const startTime = Date.now();

interface Component {
    name: string;
    theme: string;
    childCount: number;
}

// 1. Read JSON from STDIN using Javy's native I/O (File Descriptor 0)
// Allocate a buffer to hold the incoming STDIN data (e.g., 5MB)
const buffer = new Uint8Array(5 * 1024 * 1024); 
const bytesRead = Javy.IO.readSync(0, buffer);

// Decode only the exact bytes we read into a string
const rawInput = new TextDecoder().decode(buffer.subarray(0, bytesRead));
const args = JSON.parse(rawInput);

const generateCount = parseInt(args.count || "1000", 10);
const baseName = args.name || "DefaultWidget";

// 2. REAL HEAVY COMPUTE: Array Allocation & Object Creation
let components: Component[] = [];
for (let i = 0; i < generateCount; i++) {
    components.push({
        name: `${baseName}_${i}`,
        theme: i % 2 === 0 ? "dark" : "light",
        childCount: i * 3
    });
}

// 3. HEAVY STRING PROCESSING
let output = components.map(c => 
    `export const ${c.name} = () => <div className="bg-${c.theme}">Children: ${c.childCount}</div>;`
).join("\n");

const computeTime = Date.now() - startTime;

// 4. Output the metrics to the ORE Kernel (STDERR for logs, STDOUT for data)
console.error("--- TYPESCRIPT INTERNAL METRICS ---");
console.error(`Generated           : ${generateCount} React components`);
console.error(`Total String Length : ${output.length} characters`);
console.error(`Compute latency     : ${computeTime} ms`);

// Return the pure generated code to the AI Agent via STDOUT!
console.log(output.substring(0, 200) + "\n... [TRUNCATED FOR TERMINAL OUTPUT] ...");
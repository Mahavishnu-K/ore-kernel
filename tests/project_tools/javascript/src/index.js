import { groupByCategory } from "./grouper.js";

try {
    // Read STDIN via Pure WASM
    const buffer = new Uint8Array(5 * 1024 * 1024);
    const bytesRead = Javy.IO.readSync(0, buffer);
    const rawInput = new TextDecoder().decode(buffer.subarray(0, bytesRead));

    if (!rawInput) {
        throw new Error("No input provided on STDIN.");
    }

    const data = JSON.parse(rawInput);
    const result = groupByCategory(data.items);
    
    console.log(`SUCCESS (JavaScript/Lodash): Grouped data into ${Object.keys(result).length} categories.`);
    console.log(JSON.stringify(result, null, 2));
} catch (e) {
    console.error("TOOL FAILED (JavaScript):");
    console.error(e.message);
}
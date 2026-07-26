// 1. Read STDIN (File Descriptor 0) using Javy's native I/O Sandbox API
function readStdin() {
    const buffer = new Uint8Array(3 * 1024 * 1024); // Allocate a 3MB buffer in RAM
    const bytesRead = Javy.IO.readSync(0, buffer); // Read from ORE's STDIN pipe
    return new TextDecoder().decode(buffer.subarray(0, bytesRead));
}

try {
    const rawInput = readStdin();
    const data = JSON.parse(rawInput);

    if (!data.items || !Array.isArray(data.items)) {
        throw new Error("Invalid input: Expected a JSON object with an 'items' array.");
    }

    const items = data.items;
    if (items.length === 0) {
        console.log("No data provided.");
    } else {
        // 2. Dynamically generate a Markdown table
        const headers = Object.keys(items[0]);
        
        let md = "| " + headers.join(" | ") + " |\n";
        md += "| " + headers.map(() => "---").join(" | ") + " |\n";

        for (const item of items) {
            const row = headers.map(h => String(item[h] || ""));
            md += "| " + row.join(" | ") + " |\n";
        }

        // 3. Print the result to STDOUT (ORE captures this)
        console.log(md);
    }
} catch (e) {
    // Print to STDERR (ORE captures this separately!)
    console.error("Tool Error: " + e.message);
    // Throwing the error forces the Sandbox to trap and exit cleanly
    throw e;
}
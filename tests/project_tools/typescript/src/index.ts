declare const Javy: any;

import { EmployeeSchema } from "./schema.js";

try {
    const buffer = new Uint8Array(5 * 1024 * 1024); 
    const bytesRead = Javy.IO.readSync(0, buffer);

    // Decode only the exact bytes we read into a string
    const rawInput = new TextDecoder().decode(buffer.subarray(0, bytesRead));
    const data = JSON.parse(rawInput);
    
    // Zod will throw an error if the JSON is invalid!
    const validData = EmployeeSchema.parse(data);
    
    console.log(`SUCCESS (TypeScript): Employee ${validData.name} validated perfectly.`);
} catch (e: any) {
    console.error("VALIDATION FAILED (TypeScript):");
    console.error(e.message);
}
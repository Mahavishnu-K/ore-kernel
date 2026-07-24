console.log("Starting Cross-Language Execution test in TypeScript...");

// This tests basic CPU execution and stack depth inside the WASM TS runtime
function calculateFibonacci(n: number): number {
    if (n <= 1) return n;
    return calculateFibonacci(n - 1) + calculateFibonacci(n - 2);
}

const num = 30;
console.log(`Calculating Fibonacci of ${num}...`);
const start = Date.now();
const result = calculateFibonacci(num);
const end = Date.now();

console.log(`Result: ${result}`);
console.log(`Time taken: ${end - start}ms`);
console.log("SUCCESS: TypeScript cartridge executed perfectly.");

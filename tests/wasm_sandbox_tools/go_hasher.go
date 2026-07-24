package main

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"io"
	"os"
	"time"
)

func main() {
	startTime := time.Now()

	// 1. Test STDIN Pipe in Go WASI
	inputData, err := io.ReadAll(os.Stdin)
	if err != nil {
		fmt.Println("Error reading STDIN:", err)
		return
	}

	// 2. Compute SHA256 Hash
	hasher := sha256.New()
	hasher.Write(inputData)
	hashBytes := hasher.Sum(nil)
	hashString := hex.EncodeToString(hashBytes)

	computeTime := time.Since(startTime)

	// 3. Test STDOUT Pipe
	fmt.Println("--- GO INTERNAL METRICS ---")
	fmt.Printf("Processed bytes : %d\n", len(inputData))
	fmt.Printf("SHA256 Hash     : %s\n", hashString)
	fmt.Printf("Compute latency : %v\n", computeTime)
}

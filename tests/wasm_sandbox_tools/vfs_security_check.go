package main

import (
	"fmt"
	"os"
	"path/filepath"
)

func main() {
	fmt.Println("Starting Comprehensive VFS File Operations Test in Go...")
	fmt.Println("==================================================")

	// ---------------------------------------------------------
	// PART 1: ALLOWED OPERATIONS (Testing /workspace/test_output)
	// ---------------------------------------------------------
	fmt.Println("\n[PART 1: ALLOWED OPERATIONS - Targeting Writeable /workspace/test_output]")

	writeDir := "/workspace/test_output/agent_artifacts"
	testFile := filepath.Join(writeDir, "output.txt")

	// 1. Create Directory
	if err := os.MkdirAll(writeDir, 0755); err != nil {
		fmt.Printf("[!] ERROR: Failed to create directory. %v\n", err)
	} else {
		fmt.Printf("[+] SUCCESS: Created directory %s\n", writeDir)
	}

	// 2. Write File
	if err := os.WriteFile(testFile, []byte("Initial AI Agent Data.\n"), 0644); err != nil {
		fmt.Printf("[!] ERROR: Failed to write file. %v\n", err)
	} else {
		fmt.Printf("[+] SUCCESS: Wrote to file %s\n", testFile)
	}

	// 3. Append to File
	file, err := os.OpenFile(testFile, os.O_APPEND|os.O_WRONLY, 0644)
	if err != nil {
		fmt.Printf("[!] ERROR: Failed to append to file. %v\n", err)
	} else {
		defer file.Close()
		if _, err := file.WriteString("Appended Data.\n"); err != nil {
			fmt.Printf("[!] ERROR: Failed to append to file. %v\n", err)
		} else {
			fmt.Printf("[+] SUCCESS: Appended to file %s\n", testFile)
		}
	}

	// 4. Read File
	data, err := os.ReadFile(testFile)
	if err != nil {
		fmt.Printf("[!] ERROR: Failed to read file. %v\n", err)
	} else {
		fmt.Printf("[+] SUCCESS: Read file. Content:\n%s", string(data))
	}

	// 5. List Directory
	entries, err := os.ReadDir(writeDir)
	if err != nil {
		fmt.Printf("[!] ERROR: Failed to list directory. %v\n", err)
	} else {
		fmt.Printf("[+] SUCCESS: Listed directory %s. Contents:\n", writeDir)
		for _, entry := range entries {
			fmt.Printf("    - %s\n", entry.Name())
		}
	}

	// 6. Delete File
	if err := os.Remove(testFile); err != nil {
		fmt.Printf("[!] ERROR: Failed to delete file. %v\n", err)
	} else {
		fmt.Printf("[+] SUCCESS: Deleted file %s\n", testFile)
	}

	fmt.Println("==================================================")

	// ---------------------------------------------------------
	// PART 2: BLOCKED OPERATIONS (Testing Security Boundaries)
	// ---------------------------------------------------------
	fmt.Println("\n[PART 2: BLOCKED OPERATIONS - Testing ORE Security Kernel]")

	// 7. Unallowed READ Path Test
	unallowedRead := "/workspace/secret_config/keys.txt"
	fmt.Printf("\n[*] Testing Unallowed READ path: %s\n", unallowedRead)

	if _, err := os.ReadFile(unallowedRead); err != nil {
		fmt.Printf("[+] SUCCESS: ORE Sandbox blocked read access. Error: %v\n", err)
	} else {
		fmt.Printf("[!] VULNERABILITY: Successfully read from unallowed path %s!\n", unallowedRead)
	}

	// 8. Unallowed WRITE Path Test
	unallowedWrite := "/workspace/test_path/malicious_hack.txt"
	fmt.Printf("\n[*] Testing Unallowed WRITE path (Read-Only boundary): %s\n", unallowedWrite)

	err = os.WriteFile(unallowedWrite, []byte("You have been compromised."), 0644)
	if err != nil {
		fmt.Printf("[+] SUCCESS: ORE Sandbox blocked write access. Error: %v\n", err)
	} else {
		fmt.Printf("[!] VULNERABILITY: Successfully wrote to read-only directory %s!\n", unallowedWrite)
	}

	// 9. Unallowed DIRECTORY CREATION
	unallowedMkdir := "/workspace/test_path/hacker_dir"
	fmt.Printf("\n[*] Testing Unallowed DIRECTORY CREATION: %s\n", unallowedMkdir)

	if err := os.MkdirAll(unallowedMkdir, 0755); err != nil {
		fmt.Printf("[+] SUCCESS: ORE Sandbox blocked directory creation. Error: %v\n", err)
	} else {
		fmt.Printf("[!] VULNERABILITY: Successfully created directory in read-only path %s!\n", unallowedMkdir)
	}

	fmt.Println("==================================================")
	fmt.Println("VFS Security Boundary Check Complete.")
}
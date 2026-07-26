package main

import (
	"fmt"
	"io"
	"os"
	"ore_go_tool/parser"
)

func main() {
	inputBytes, _ := io.ReadAll(os.Stdin)
	jsonStr := string(inputBytes)

	names := parser.GetNames(jsonStr)
	fmt.Printf("SUCCESS (Go/gjson): Extracted %d names from massive JSON -> %v\n", len(names), names)
}
// Command hostcontract derives the host command contract artifacts that Wails cannot generate, and checks the
// source for violations of the contract rules. Wails itself generates the command signatures, DTOs and event
// types (`wails3 generate bindings`); see scripts/gen-host-bindings.mjs for that step.
//
//	go run ./cmd/hostcontract errors-ts [-check]   # frontend form of the error catalog
//	go run ./cmd/hostcontract lint                 # static rules on the Go sources
//
// Paths are resolved from the repository root (two levels above apps/gui-go).
package main

import (
	"bytes"
	"fmt"
	"os"
	"path/filepath"
)

const errorsTSPath = "apps/gui/src/lib/host-errors.generated.ts"

func repoRoot() string {
	dir, err := os.Getwd()
	if err != nil {
		fatal(err)
	}
	for {
		if _, err := os.Stat(filepath.Join(dir, "apps", "gui-go", "go.mod")); err == nil {
			return dir
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			fatal(fmt.Errorf("repository root not found from %s", dir))
		}
		dir = parent
	}
}

func fatal(err error) {
	fmt.Fprintln(os.Stderr, "hostcontract:", err)
	os.Exit(1)
}

// writeOrCheck writes the generated file, or with check compares it with the committed one.
func writeOrCheck(path, content string, check bool) {
	if check {
		current, err := os.ReadFile(path)
		if err != nil || !bytes.Equal(current, []byte(content)) {
			fatal(fmt.Errorf("%s is stale: run `bun run gen:host-contract` and commit the result", path))
		}
		return
	}
	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		fatal(err)
	}
}

func main() {
	if len(os.Args) < 2 {
		fatal(fmt.Errorf("usage: hostcontract errors-ts|lint [-check]"))
	}
	root := repoRoot()
	check := len(os.Args) > 2 && os.Args[2] == "-check"
	switch os.Args[1] {
	case "errors-ts":
		writeOrCheck(filepath.Join(root, errorsTSPath), renderErrorsTS(), check)
	default:
		fatal(fmt.Errorf("unknown subcommand %q", os.Args[1]))
	}
}

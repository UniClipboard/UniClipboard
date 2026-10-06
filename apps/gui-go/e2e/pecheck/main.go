// Command pecheck validates the structure and architecture of a Windows executable with the standard library's
// debug/pe: `pecheck <amd64|arm64> <file>`. Exit 0 prints a JSON summary; otherwise the reason goes to stderr.
// It proves the file is a well-formed PE executable (not a DLL) for the architecture. It says nothing about where the
// file came from, whether it is the Rust daemon, or whether it runs.
package main

import (
	"debug/pe"
	"encoding/json"
	"fmt"
	"os"
)

func main() {
	if len(os.Args) != 3 {
		fmt.Fprintln(os.Stderr, "usage: pecheck <amd64|arm64> <file>")
		os.Exit(2)
	}
	want := map[string]uint16{"amd64": pe.IMAGE_FILE_MACHINE_AMD64, "arm64": pe.IMAGE_FILE_MACHINE_ARM64}[os.Args[1]]
	if want == 0 {
		fmt.Fprintln(os.Stderr, "unknown architecture", os.Args[1])
		os.Exit(2)
	}
	f, err := pe.Open(os.Args[2])
	if err != nil {
		fmt.Fprintln(os.Stderr, "not a well-formed PE file:", err)
		os.Exit(1)
	}
	defer f.Close()
	const executable, dll = 0x0002, 0x2000
	switch {
	case f.Machine != want:
		fmt.Fprintf(os.Stderr, "PE machine 0x%04X, expected 0x%04X for %s\n", f.Machine, want, os.Args[1])
		os.Exit(1)
	case f.Characteristics&executable == 0 || f.Characteristics&dll != 0:
		fmt.Fprintf(os.Stderr, "not an executable image (characteristics 0x%04X)\n", f.Characteristics)
		os.Exit(1)
	case len(f.Sections) == 0:
		fmt.Fprintln(os.Stderr, "PE file has no sections")
		os.Exit(1)
	}
	_ = json.NewEncoder(os.Stdout).Encode(map[string]any{"machine": fmt.Sprintf("0x%04X", f.Machine), "sections": len(f.Sections)})
}

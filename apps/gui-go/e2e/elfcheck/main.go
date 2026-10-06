// Command elfcheck validates the structure and architecture of a Linux executable with the standard library's
// debug/elf: `elfcheck <amd64|arm64> <file>`. Exit 0 prints a JSON summary; otherwise the reason goes to stderr.
// It proves the file is a well-formed ELF executable (or PIE) for the architecture. It says nothing about where the
// file came from, whether it is the Rust daemon, or whether it runs.
package main

import (
	"debug/elf"
	"encoding/json"
	"fmt"
	"os"
)

func main() {
	if len(os.Args) != 3 {
		fmt.Fprintln(os.Stderr, "usage: elfcheck <amd64|arm64> <file>")
		os.Exit(2)
	}
	want, ok := map[string]elf.Machine{"amd64": elf.EM_X86_64, "arm64": elf.EM_AARCH64}[os.Args[1]]
	if !ok {
		fmt.Fprintln(os.Stderr, "unknown architecture", os.Args[1])
		os.Exit(2)
	}
	f, err := elf.Open(os.Args[2])
	if err != nil {
		fmt.Fprintln(os.Stderr, "not a well-formed ELF file:", err)
		os.Exit(1)
	}
	defer f.Close()
	switch {
	case f.Machine != want:
		fmt.Fprintf(os.Stderr, "ELF machine %v, expected %v for %s\n", f.Machine, want, os.Args[1])
		os.Exit(1)
	case f.Type != elf.ET_EXEC && f.Type != elf.ET_DYN:
		fmt.Fprintf(os.Stderr, "not an executable image (type %v)\n", f.Type)
		os.Exit(1)
	case f.Class != elf.ELFCLASS64 || f.Data != elf.ELFDATA2LSB:
		fmt.Fprintln(os.Stderr, "not a 64-bit little-endian ELF")
		os.Exit(1)
	case len(f.Sections) == 0:
		fmt.Fprintln(os.Stderr, "ELF file has no sections")
		os.Exit(1)
	}
	_ = json.NewEncoder(os.Stdout).Encode(map[string]any{"machine": f.Machine.String(), "type": f.Type.String(), "sections": len(f.Sections)})
}

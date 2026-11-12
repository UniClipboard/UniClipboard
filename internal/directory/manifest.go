// Package directory defines portable, bounded tree content metadata.
package directory

import (
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"strings"
	"unicode"
	"unicode/utf8"

	"golang.org/x/text/cases"
	"golang.org/x/text/unicode/norm"
)

const (
	MIME          = "application/vnd.unic.directory+json"
	ReferenceMIME = "application/vnd.unic.directory-reference"
	MaxRoots      = 128
	MaxEntries    = 4096
	MaxFiles      = 2048
	MaxDepth      = 64
	MaxManifest   = 512 << 10
)

type Root struct {
	Name      string `json:"name"`
	Directory bool   `json:"directory"`
}
type Entry struct {
	Root      int    `json:"root"`
	Path      string `json:"path"`
	Directory bool   `json:"directory"`
	File      int    `json:"file"`
	Size      int64  `json:"size"`
	SHA256    string `json:"sha256,omitempty"`
}
type Manifest struct {
	Roots   []Root  `json:"roots"`
	Entries []Entry `json:"entries"`
}

func Key(s string) string { return cases.Fold().String(norm.NFC.String(s)) }
func ValidName(s string) bool {
	if s == "" || s == "." || s == ".." || len(s) > 255 || !utf8.ValidString(s) || strings.TrimRight(s, " .") != s {
		return false
	}
	for _, r := range s {
		if unicode.IsControl(r) || strings.ContainsRune("/\\:*?\"<>|", r) {
			return false
		}
	}
	base := strings.ToUpper(strings.SplitN(s, ".", 2)[0])
	switch base {
	case "CON", "PRN", "AUX", "NUL":
		return false
	}
	if len(base) == 4 && (strings.HasPrefix(base, "COM") || strings.HasPrefix(base, "LPT")) && base[3] >= '0' && base[3] <= '9' {
		return false
	}
	return true
}
func Decode(data []byte, sizes []int64) (*Manifest, error) {
	if len(data) == 0 || len(data) > MaxManifest || !utf8.Valid(data) {
		return nil, errors.New("invalid manifest size")
	}
	var m Manifest
	dec := json.NewDecoder(strings.NewReader(string(data)))
	dec.DisallowUnknownFields()
	if err := dec.Decode(&m); err != nil {
		return nil, errors.New("invalid manifest")
	}
	// Reject trailing JSON values and malformed suffixes.
	var extra any
	if err := dec.Decode(&extra); err != io.EOF {
		return nil, errors.New("invalid manifest suffix")
	}
	if err := m.Validate(sizes); err != nil {
		return nil, err
	}
	return &m, nil
}
func (m *Manifest) Validate(sizes []int64) error {
	bad := func() error { return errors.New("invalid directory structure") }
	if len(m.Roots) == 0 || len(m.Roots) > MaxRoots || len(m.Entries) == 0 || len(m.Entries) > MaxEntries || len(sizes) > MaxFiles {
		return bad()
	}
	for _, r := range m.Roots {
		if !ValidName(r.Name) {
			return bad()
		}
	}
	seen := map[string]bool{}
	files := make([]bool, len(sizes))
	rootEntries := make([]bool, len(m.Roots))
	for _, e := range m.Entries {
		if e.Root < 0 || e.Root >= len(m.Roots) || len(e.Path) > 4096 {
			return bad()
		}
		parts := strings.Split(e.Path, "/")
		if len(parts) > MaxDepth {
			return bad()
		}
		if e.Path != "" {
			for _, p := range parts {
				if !ValidName(p) {
					return bad()
				}
			}
		}
		k := fmt.Sprintf("%d/%s", e.Root, Key(e.Path))
		if _, ok := seen[k]; ok {
			return bad()
		}
		if e.Path == "" {
			rootEntries[e.Root] = true
			if m.Roots[e.Root].Directory != e.Directory {
				return bad()
			}
		} else {
			parent := ""
			if n := strings.LastIndex(e.Path, "/"); n >= 0 {
				parent = e.Path[:n]
			}
			if !seen[fmt.Sprintf("%d/%s", e.Root, Key(parent))] {
				return bad()
			}
		}
		seen[k] = e.Directory
		if e.Directory {
			if e.Size != 0 || e.SHA256 != "" {
				return bad()
			}
			continue
		}
		if e.File < 0 || e.File >= len(sizes) || files[e.File] || e.Size != sizes[e.File] || e.Size < 0 {
			return bad()
		}
		h, err := hex.DecodeString(e.SHA256)
		if err != nil || len(h) != 32 || strings.ToLower(e.SHA256) != e.SHA256 {
			return bad()
		}
		files[e.File] = true
	}
	for _, v := range rootEntries {
		if !v {
			return bad()
		}
	}
	for _, v := range files {
		if !v {
			return bad()
		}
	}
	return nil
}

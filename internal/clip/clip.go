// Package clip models a clipboard snapshot independently of the system
// clipboard, so the sync logic can be tested without a GUI session.
package clip

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"sort"
)

// Common format names. They match mygo's transfer package (MIME types).
const (
	Text = "text/plain"
	HTML = "text/html"
	PNG  = "image/png"
)

// Part is one in-memory representation (text, HTML, PNG, ...).
type Part struct {
	Format string
	Data   []byte
}

// File is a file on the clipboard. Path is absolute and local to this device.
type File struct {
	Name string // base name, as the sender called it
	Path string
	Size int64
}

// Snapshot is the content of the clipboard at one moment.
type Snapshot struct {
	Parts []Part
	Files []File
}

// Empty reports whether the snapshot carries nothing.
func (s Snapshot) Empty() bool { return len(s.Parts) == 0 && len(s.Files) == 0 }

// Hash identifies the content: equal content gives an equal hash on every
// device, which is how echoes are told apart from real changes. Files are
// identified by name and size, not by content, to keep polling cheap.
func (s Snapshot) Hash() string {
	if s.Empty() {
		return ""
	}
	parts := append([]Part(nil), s.Parts...)
	sort.Slice(parts, func(i, j int) bool { return parts[i].Format < parts[j].Format })
	h := sha256.New()
	for _, p := range parts {
		_, _ = fmt.Fprintf(h, "P%d:%s:%d:", len(p.Format), p.Format, len(p.Data))
		h.Write(p.Data)
	}
	files := append([]File(nil), s.Files...)
	sort.Slice(files, func(i, j int) bool { return files[i].Name < files[j].Name })
	for _, f := range files {
		_, _ = fmt.Fprintf(h, "F%d:%s:%d:", len(f.Name), f.Name, f.Size)
	}
	return hex.EncodeToString(h.Sum(nil))
}

// Summary is a short human-readable description for logs, CLI and GUI.
func (s Snapshot) Summary() string {
	if len(s.Files) > 0 {
		if len(s.Files) == 1 {
			return "file " + s.Files[0].Name
		}
		return fmt.Sprintf("%d files", len(s.Files))
	}
	for _, p := range s.Parts {
		if p.Format == PNG {
			return fmt.Sprintf("image (%d bytes)", len(p.Data))
		}
	}
	for _, p := range s.Parts {
		if p.Format == Text {
			r := []rune(string(p.Data))
			if len(r) > 40 {
				return fmt.Sprintf("text %q…", string(r[:40]))
			}
			return fmt.Sprintf("text %q", string(r))
		}
	}
	if len(s.Parts) > 0 {
		return s.Parts[0].Format
	}
	return "empty"
}

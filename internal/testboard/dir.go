package testboard

import (
	"errors"
	"os"
	"path/filepath"
	"strings"

	"github.com/uniclipboard/unic/internal/clip"
)

// Dir is a clipboard kept in a directory instead of the system: one file per
// representation (text, html, png) and a files list with one absolute path
// per line. It exists so end-to-end tests can run several daemons on one
// machine without sharing, or overwriting, the user's real clipboard; a test
// changes the clipboard by writing these files and reads it back the same way.
type Dir string

// dirFormats maps a file name to the representation it carries, in read order.
var dirFormats = []struct{ file, format string }{
	{"text", "text/plain"},
	{"html", "text/html"},
	{"png", "image/png"},
}

const dirFiles = "files"

func (d Dir) Read() (clip.Snapshot, error) {
	var s clip.Snapshot
	if b, err := os.ReadFile(filepath.Join(string(d), dirFiles)); err == nil {
		for _, p := range strings.Split(string(b), "\n") {
			if p = strings.TrimSpace(p); p == "" {
				continue
			}
			f := clip.File{Name: filepath.Base(p), Path: p}
			if st, err := os.Stat(p); err == nil {
				f.Size = st.Size()
			}
			s.Files = append(s.Files, f)
		}
		if len(s.Files) > 0 {
			return s, nil // files win over text, as on the system clipboard
		}
	}
	for _, f := range dirFormats {
		b, err := os.ReadFile(filepath.Join(string(d), f.file))
		if err != nil || len(b) == 0 {
			continue
		}
		s.Parts = append(s.Parts, clip.Part{Format: f.format, Data: b})
	}
	return s, nil
}

func (d Dir) Write(s clip.Snapshot) error {
	if err := os.MkdirAll(string(d), 0o700); err != nil {
		return err
	}
	for _, name := range []string{dirFiles, "text", "html", "png"} {
		if err := os.Remove(filepath.Join(string(d), name)); err != nil && !errors.Is(err, os.ErrNotExist) {
			return err
		}
	}
	if len(s.Files) > 0 {
		var b strings.Builder
		for _, f := range s.Files {
			b.WriteString(f.Path + "\n")
		}
		return writeAtomic(filepath.Join(string(d), dirFiles), []byte(b.String()))
	}
	for _, p := range s.Parts {
		for _, f := range dirFormats {
			if f.format == p.Format {
				if err := writeAtomic(filepath.Join(string(d), f.file), p.Data); err != nil {
					return err
				}
			}
		}
	}
	return nil
}

// writeAtomic keeps a poller from reading a half-written file.
func writeAtomic(path string, data []byte) error {
	tmp := path + ".tmp"
	if err := os.WriteFile(tmp, data, 0o600); err != nil {
		return err
	}
	return os.Rename(tmp, path)
}

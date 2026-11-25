package testboard

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/uniclipboard/unic/internal/clip"
	"github.com/uniclipboard/unic/sourceapp"
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

var dirLocks sync.Map // map[string]*sync.Mutex

func dirMutex(dir string) *sync.Mutex {
	key := filepath.Clean(dir)
	v, _ := dirLocks.LoadOrStore(key, &sync.Mutex{})
	return v.(*sync.Mutex)
}

func (d Dir) Read() (clip.Snapshot, error) {
	if err := d.waitDirectoryApply(); err != nil {
		return clip.Snapshot{}, err
	}
	mu := dirMutex(string(d))
	mu.Lock()
	defer mu.Unlock()

	var s clip.Snapshot
	if data, err := os.ReadFile(filepath.Join(string(d), "source.json")); err == nil {
		s.SourceApplication = sourceapp.Decode(data)
	}
	if b, err := os.ReadFile(filepath.Join(string(d), dirFiles)); err == nil {
		if st, err := os.Stat(filepath.Join(string(d), dirFiles)); err == nil {
			s.CopyID = fmt.Sprint(st.ModTime().UnixNano())
		}
		for _, p := range strings.Split(string(b), "\n") {
			if p = strings.TrimSpace(p); p == "" {
				continue
			}
			f := clip.File{Name: filepath.Base(p), Path: p}
			if st, err := os.Stat(p); err == nil {
				f.Size = st.Size()
				f.Directory = st.IsDir()
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
	mu := dirMutex(string(d))
	mu.Lock()
	defer mu.Unlock()

	kept := map[string]bool{}
	if len(s.Files) > 0 {
		var b strings.Builder
		for _, f := range s.Files {
			b.WriteString(f.Path + "\n")
		}
		if err := writeAtomic(filepath.Join(string(d), dirFiles), []byte(b.String())); err != nil {
			return err
		}
		kept[dirFiles] = true
	} else {
		for _, p := range s.Parts {
			for _, f := range dirFormats {
				if f.format == p.Format {
					if err := writeAtomic(filepath.Join(string(d), f.file), p.Data); err != nil {
						return err
					}
					kept[f.file] = true
				}
			}
		}
	}
	for _, name := range []string{dirFiles, "text", "html", "png", "source.json"} {
		if kept[name] {
			continue
		}
		if err := os.Remove(filepath.Join(string(d), name)); err != nil && !errors.Is(err, os.ErrNotExist) {
			return err
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

// waitDirectoryApply is an opt-in filesystem gate for isolated process acceptance.
// Ordinary reads stay unchanged. No production build uses this fixture board.
func (d Dir) waitDirectoryApply() error {
	hold := filepath.Join(string(d), ".hold-directory-apply")
	if _, err := os.Stat(hold); err != nil {
		return nil
	}
	inbox := filepath.Join(filepath.Dir(string(d)), "inbox")
	entries, err := os.ReadDir(inbox)
	if err != nil {
		return nil
	}
	materialized := false
	for _, e := range entries {
		if e.IsDir() && strings.HasPrefix(e.Name(), "unic-content-") {
			if info, err := os.Stat(filepath.Join(inbox, e.Name(), "view")); err == nil && info.IsDir() {
				materialized = true
				break
			}
		}
	}
	if !materialized {
		return nil
	}
	if err := os.WriteFile(filepath.Join(string(d), ".directory-apply-held"), []byte("final clipboard read"), 0600); err != nil {
		return err
	}
	end := time.Now().Add(15 * time.Second)
	for time.Now().Before(end) {
		if _, err := os.Stat(hold); errors.Is(err, os.ErrNotExist) {
			return nil
		}
		time.Sleep(5 * time.Millisecond)
	}
	return errors.New("fixture directory apply gate expired")
}

// WatchChanges notifies fn whenever the directory clipboard content changes.
// It is used only in isolated desktop_e2e runs while a panel file lease is held
// and stops immediately when the returned stop function is called.
func (d Dir) WatchChanges(fn func()) func() {
	done := make(chan struct{})
	var once sync.Once
	stop := func() { once.Do(func() { close(done) }) }
	initial, _ := d.Read()
	last := initial.ObservationHash()
	go func() {
		ticker := time.NewTicker(80 * time.Millisecond)
		defer ticker.Stop()
		for {
			select {
			case <-done:
				return
			case <-ticker.C:
				cur, err := d.Read()
				if err != nil {
					continue
				}
				h := cur.ObservationHash()
				if h != last {
					last = h
					fn()
				}
			}
		}
	}()
	return stop
}

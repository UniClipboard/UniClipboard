package localcontent

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"reflect"
	"strings"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/directory"
)

const directoryPrefix = "directory-source:"

type directoryRef struct {
	Path     string
	Identity string
}
type DirectorySource struct {
	ref  directoryRef
	seen map[string]os.FileInfo
}

func sourceFailure(code engine.Code) error {
	return &engine.Error{Code: code, Message: "directory source is unavailable or unsupported"}
}
func identity(st os.FileInfo) string {
	// Unix file identity is stable across directory edits. Other platforms use
	// creation identity when supplied by os.Stat, never directory byte size.
	v := reflect.Indirect(reflect.ValueOf(st.Sys()))
	var s string
	if v.IsValid() && v.Kind() == reflect.Struct {
		for _, k := range []string{"Dev", "Ino", "CreationTime"} {
			f := v.FieldByName(k)
			if f.IsValid() && f.CanInterface() {
				s += fmt.Sprint(f.Interface()) + ":"
			}
		}
	}
	if s == "" {
		s = fmt.Sprint(st.ModTime().UnixNano())
	}
	return s
}
func NewDirectorySource(path string, st os.FileInfo) *DirectorySource {
	return &DirectorySource{ref: directoryRef{path, identity(st)}}
}
func (d *DirectorySource) PersistentRef() string {
	b, _ := json.Marshal(d.ref)
	return directoryPrefix + base64.RawURLEncoding.EncodeToString(b)
}
func ResolveDirectory(ref string) engine.FileSource {
	b, err := base64.RawURLEncoding.DecodeString(strings.TrimPrefix(ref, directoryPrefix))
	if err != nil {
		return nil
	}
	var r directoryRef
	if json.Unmarshal(b, &r) != nil || !filepath.IsAbs(r.Path) || r.Identity == "" {
		return nil
	}
	return &DirectorySource{ref: r}
}
func DirectoryPath(ref string) (string, bool) {
	s := ResolveDirectory(ref)
	if s == nil {
		return "", false
	}
	return s.(*DirectorySource).ref.Path, true
}
func IsDirectoryRef(ref string) bool { return strings.HasPrefix(ref, directoryPrefix) }
func (d *DirectorySource) Open(ctx context.Context) (io.ReadCloser, engine.FileMeta, error) {
	if err := ctx.Err(); err != nil {
		return nil, engine.FileMeta{}, err
	}
	st, err := os.Lstat(d.ref.Path)
	if err != nil || !st.IsDir() || st.Mode()&os.ModeSymlink != 0 || identity(st) != d.ref.Identity {
		return nil, engine.FileMeta{}, sourceFailure(engine.CodeSourceChanged)
	}
	return io.NopCloser(strings.NewReader("")), engine.FileMeta{Version: d.ref.Identity}, nil
}
func (d *DirectorySource) entries(ctx context.Context) ([]engine.DirectoryEntry, error) {
	rc, _, err := d.Open(ctx)
	if err != nil {
		return nil, err
	}
	_ = rc.Close()
	root, err := os.OpenRoot(d.ref.Path)
	if err != nil {
		return nil, sourceFailure(engine.CodePermissionDenied)
	}
	defer func() { _ = root.Close() }()
	d.seen = map[string]os.FileInfo{}
	var out []engine.DirectoryEntry
	var walk func(string) error
	walk = func(path string) error {
		if err := ctx.Err(); err != nil {
			return err
		}
		st, err := root.Lstat(path)
		if err != nil {
			return sourceFailure(engine.CodePermissionDenied)
		}
		if st.Mode()&os.ModeSymlink != 0 || (!st.IsDir() && !st.Mode().IsRegular()) {
			return sourceFailure(engine.CodeUnsupported)
		}
		rel := filepath.ToSlash(path)
		if rel == "." {
			rel = ""
		}
		if len(out) >= directory.MaxEntries || len(strings.Split(rel, "/")) > directory.MaxDepth || len(rel) > 4096 {
			return sourceFailure(engine.CodeLimitExceeded)
		}
		entry := engine.DirectoryEntry{Path: rel, Directory: st.IsDir(), Size: st.Size()}
		if entry.Directory {
			entry.Size = 0
		} else {
			entry.Source = &treeFileSource{rootPath: d.ref.Path, rel: path, expected: st}
		}
		d.seen[path] = st
		out = append(out, entry)
		if !st.IsDir() {
			return nil
		}
		f, err := root.Open(path)
		if err != nil {
			return sourceFailure(engine.CodePermissionDenied)
		}
		defer func() { _ = f.Close() }()
		for {
			entries, err := f.ReadDir(64)
			for _, e := range entries {
				if err := walk(filepath.Join(path, e.Name())); err != nil {
					return err
				}
			}
			if err == io.EOF {
				return nil
			}
			if err != nil {
				return sourceFailure(engine.CodePermissionDenied)
			}
		}
	}
	err = walk(".")
	if err != nil {
		return nil, err
	}
	return out, nil
}
func (d *DirectorySource) verify(ctx context.Context) error {
	root, err := os.OpenRoot(d.ref.Path)
	if err != nil {
		return sourceFailure(engine.CodeSourceChanged)
	}
	defer func() { _ = root.Close() }()
	for path, old := range d.seen {
		if err := ctx.Err(); err != nil {
			return err
		}
		st, e := root.Lstat(path)
		if e != nil || !os.SameFile(old, st) || old.Size() != st.Size() || !old.ModTime().Equal(st.ModTime()) {
			return sourceFailure(engine.CodeSourceChanged)
		}
	}
	return nil
}

type treeFileSource struct {
	rootPath, rel string
	expected      os.FileInfo
}

func (s *treeFileSource) Open(ctx context.Context) (io.ReadCloser, engine.FileMeta, error) {
	if err := ctx.Err(); err != nil {
		return nil, engine.FileMeta{}, err
	}
	root, err := os.OpenRoot(s.rootPath)
	if err != nil {
		return nil, engine.FileMeta{}, sourceFailure(engine.CodeSourceChanged)
	}
	st, err := root.Lstat(s.rel)
	if err != nil || !st.Mode().IsRegular() || !os.SameFile(s.expected, st) || st.Size() != s.expected.Size() || !st.ModTime().Equal(s.expected.ModTime()) {
		_ = root.Close()
		return nil, engine.FileMeta{}, sourceFailure(engine.CodeSourceChanged)
	}
	f, err := root.Open(s.rel)
	_ = root.Close()
	if err != nil {
		return nil, engine.FileMeta{}, sourceFailure(engine.CodePermissionDenied)
	}
	opened, err := f.Stat()
	if err != nil || !os.SameFile(st, opened) || opened.Size() != st.Size() || !opened.ModTime().Equal(st.ModTime()) {
		_ = f.Close()
		return nil, engine.FileMeta{}, sourceFailure(engine.CodeSourceChanged)
	}
	return f, engine.FileMeta{Size: st.Size(), Version: identity(st) + fmt.Sprint(st.ModTime().UnixNano())}, nil
}

func (d *DirectorySource) ReadDirectory(ctx context.Context) (engine.DirectoryListing, error) {
	scan := &DirectorySource{ref: d.ref}
	entries, err := scan.entries(ctx)
	if err != nil {
		return engine.DirectoryListing{}, err
	}
	return engine.DirectoryListing{Entries: entries, Verify: scan.verify}, nil
}

package localcontent

import (
	"context"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/clip"
	"github.com/uniclipboard/unic/internal/directory"
)

// pathSource keeps a file where it is: the engine opens it again whenever it
// needs the bytes and compares size and modification time with what it saw.
type PathSource struct{ Path string }

// PersistentRef lets history remember the file by path instead of copying it.
func (p PathSource) PersistentRef() string { return p.Path }

func (p PathSource) Open(ctx context.Context) (io.ReadCloser, engine.FileMeta, error) {
	if err := ctx.Err(); err != nil {
		return nil, engine.FileMeta{}, err
	}
	before, err := os.Lstat(p.Path)
	if err != nil {
		return nil, engine.FileMeta{}, err
	}
	if !before.Mode().IsRegular() {
		return nil, engine.FileMeta{}, sourceFailure(engine.CodeSourceChanged)
	}
	f, err := os.Open(p.Path)
	if err != nil {
		return nil, engine.FileMeta{}, err
	}
	st, err := f.Stat()
	if err != nil || !st.Mode().IsRegular() || !os.SameFile(before, st) {
		_ = f.Close()
		return nil, engine.FileMeta{}, errors.New("not a regular file")
	}
	return f, engine.FileMeta{Size: st.Size(), Version: fmt.Sprint(st.ModTime().UnixNano())}, nil
}

// errUnrepresentable means a clipboard snapshot has nothing the engine can
// carry (a folder, a vanished file, only vendor formats).
var errUnrepresentable = errors.New("clipboard content cannot be synced")

// contentInput converts a clipboard snapshot. Only the formats that travel
// between devices are kept; files are referenced, not copied.
func Input(s clip.Snapshot) (engine.ContentInput, error) {
	in := engine.ContentInput{ObservationKey: s.CopyID, SourceApplication: s.SourceApplication.Clone()}
	if len(s.Files) > directory.MaxRoots {
		return in, &engine.Error{Code: engine.CodeLimitExceeded, Message: "too many selected roots"}
	}
	var selected []string
	for _, f := range s.Files {
		path := f.Path
		if IsDirectoryRef(path) {
			var ok bool
			path, ok = DirectoryPath(path)
			if !ok {
				return in, errUnrepresentable
			}
		}
		canonical, err := filepath.EvalSymlinks(path)
		if err != nil {
			return in, errUnrepresentable
		}
		for _, old := range selected {
			if canonical == old || strings.HasPrefix(canonical, old+string(os.PathSeparator)) || strings.HasPrefix(old, canonical+string(os.PathSeparator)) {
				return in, &engine.Error{Code: engine.CodeInvalidArgument, Message: "selected roots overlap"}
			}
		}
		selected = append(selected, canonical)
	}

	for _, p := range s.Parts {
		switch p.Format {
		case clip.Text, clip.HTML, clip.PNG:
		default:
			continue
		}
		if len(p.Data) == 0 {
			continue
		}
		in.Representations = append(in.Representations, engine.RepresentationInput{MIME: p.Format, Size: int64(len(p.Data)), Reader: bytesReader(p.Data)})
	}
	for _, f := range s.Files {
		if !filepath.IsAbs(f.Path) && !IsDirectoryRef(f.Path) {
			return in, errUnrepresentable
		}
		if IsDirectoryRef(f.Path) {
			source := ResolveDirectory(f.Path)
			if source == nil {
				return in, errUnrepresentable
			}
			in.Files = append(in.Files, engine.FileInput{Name: f.Name, MIME: directory.ReferenceMIME, Source: source})
			continue
		}
		st, err := os.Lstat(f.Path)
		if err == nil && st.IsDir() {
			in.Files = append(in.Files, engine.FileInput{Name: f.Name, MIME: directory.ReferenceMIME, Source: NewDirectorySource(f.Path, st)})
			continue
		}
		if err != nil || !st.Mode().IsRegular() {
			return in, errUnrepresentable // a folder or a vanished file: syncing its name as text would mislead
		}
		in.Files = append(in.Files, engine.FileInput{Name: f.Name, Size: st.Size(), Source: PathSource{f.Path}})
	}
	if len(in.Representations)+len(in.Files) == 0 {
		return in, errUnrepresentable
	}
	return in, nil
}

func bytesReader(b []byte) io.Reader { return strings.NewReader(string(b)) }

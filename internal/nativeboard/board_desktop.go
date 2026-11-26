//go:build linux || windows

package nativeboard

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"time"

	"golang.design/x/clipboard"

	"github.com/uniclipboard/unic/internal/clip"
)

const maxRepresentation = 64 << 20

// Board uses the native clipboard of the daemon's interactive desktop.
type Board struct {
	read  func() (clip.Snapshot, error)
	write func(clip.Snapshot) error
}

func New() (*Board, error) {
	if err := checkDesktopSession(); err != nil {
		return nil, err
	}
	if b := platformClipboard(); b != nil {
		return b, nil
	}
	if err := clipboard.Init(); err != nil {
		return nil, ErrDesktopClipboardUnsupported
	}
	return &Board{}, nil
}

func (b *Board) Read() (clip.Snapshot, error) {
	if b.read != nil {
		return b.read()
	}
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	revision, app := ClipboardSource()
	generation := b.ClipboardGeneration()
	out := clip.Snapshot{CopyID: generation, SourceApplication: app}
	paths, err := clipboard.ReadFiles(ctx)
	if err != nil && !errors.Is(err, clipboard.ErrNoData) {
		return clip.Snapshot{}, err
	}
	out.Files = clipboardFiles(paths)
	if len(out.Files) == 0 {
		for _, spec := range desktopFormats {
			data, err := spec.read(ctx)
			if errors.Is(err, clipboard.ErrNoData) {
				continue
			}
			if err != nil {
				return clip.Snapshot{}, err
			}
			if len(data) > 0 && len(data) <= maxRepresentation {
				out.Parts = append(out.Parts, clip.Part{Format: spec.mime, Data: data})
			}
		}
	}
	if revision != ClipboardOwnerRevision() || generation != b.ClipboardGeneration() {
		return clip.Snapshot{}, errors.New("clipboard changed during read")
	}
	return out, nil
}

func clipboardFiles(paths []string) []clip.File {
	var files []clip.File
	for _, path := range paths {
		path = filepath.Clean(path)
		if !filepath.IsAbs(path) {
			continue
		}
		file := clip.File{Path: path, Name: filepath.Base(path)}
		if st, err := os.Stat(path); err == nil {
			file.Size, file.Directory = st.Size(), st.IsDir()
		}
		files = append(files, file)
	}
	return files
}

func (b *Board) Write(s clip.Snapshot) error {
	if b.write != nil {
		return b.write(s)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	if len(s.Files) > 0 {
		paths := make([]string, 0, len(s.Files))
		for _, f := range s.Files {
			paths = append(paths, f.Path)
		}
		_, err := clipboard.WriteFiles(ctx, paths)
		return err
	}
	options := make([]clipboard.Option, 0, len(s.Parts))
	for _, part := range s.Parts {
		for _, spec := range desktopFormats {
			if part.Format == spec.mime {
				options = spec.appendItems(options, part.Data)
				break
			}
		}
	}
	_, err := clipboard.WriteAll(ctx, options...)
	return err
}

type desktopFormat struct {
	mime      string
	format    clipboard.Format
	alternate clipboard.Format
	decode    func([]byte) []byte
	encode    func([]byte) []byte
}

func (f desktopFormat) read(ctx context.Context) ([]byte, error) {
	data, err := clipboard.Read(ctx, f.format)
	if f.alternate != 0 && errors.Is(err, clipboard.ErrNoData) {
		return clipboard.Read(ctx, f.alternate)
	}
	if err == nil && f.decode != nil {
		data = f.decode(data)
	}
	return data, err
}

func (f desktopFormat) appendItems(items []clipboard.Option, data []byte) []clipboard.Option {
	if f.encode != nil {
		data = f.encode(data)
	}
	items = append(items, clipboard.Item{Format: f.format, Bytes: data})
	if f.mime == clip.PNG && f.alternate != 0 {
		items = append(items, clipboard.Item{Format: f.alternate, Bytes: data})
	}
	return items
}

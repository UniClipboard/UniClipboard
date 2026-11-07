// Package board adapts mygo's clipboard to clip.Board.
package board

import (
	"os"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/transfer"

	"uni/internal/clip"
)

type MyGo struct{}

// syncFormats are the representations that travel between devices, in the
// order they are read. Everything else (vendor formats, TIFF twins of a PNG)
// is local to the application that put it there.
var syncFormats = []transfer.Format{transfer.Text, transfer.HTML, transfer.PNG}

func (MyGo) Read() (clip.Snapshot, error) {
	var s clip.Snapshot
	offered := mygo.Clipboard.Formats()
	has := func(f transfer.Format) bool {
		for _, o := range offered {
			if o == f {
				return true
			}
		}
		return false
	}

	// Files win over the text some file managers put next to them (the name).
	if has(transfer.FileList) || has(transfer.URIList) {
		if paths, err := mygo.Clipboard.ReadFiles(); err == nil && len(paths) > 0 {
			for _, p := range paths {
				f := clip.File{Name: baseName(p), Path: p}
				// A path that cannot be stat'ed keeps Size 0 and is rejected
				// by the engine, which then syncs nothing.
				if st, err := os.Stat(p); err == nil {
					f.Size = st.Size()
				}
				s.Files = append(s.Files, f)
			}
			return s, nil
		}
	}

	var want []transfer.Format
	for _, f := range syncFormats {
		if has(f) {
			want = append(want, f)
		}
	}
	if len(want) == 0 {
		return s, nil
	}
	d, err := mygo.Clipboard.Read(want...)
	if err != nil {
		return s, err
	}
	for _, f := range want {
		b, err := d.Read(f)
		if err != nil || len(b) == 0 {
			continue
		}
		s.Parts = append(s.Parts, clip.Part{Format: string(f), Data: b})
	}
	return s, nil
}

func (MyGo) Write(s clip.Snapshot) error {
	if len(s.Files) > 0 {
		paths := make([]string, len(s.Files))
		for i, f := range s.Files {
			paths[i] = f.Path
		}
		return mygo.Clipboard.WriteFiles(paths...)
	}
	// One item with several representations: they are alternatives for the
	// same content, as a rich-text copy offers text, HTML and an image.
	reps := make([]transfer.Representation, 0, len(s.Parts))
	for _, p := range s.Parts {
		reps = append(reps, transfer.Bytes(transfer.Format(p.Format), p.Data))
	}
	return mygo.Clipboard.Write(transfer.New(transfer.NewItem(reps...)))
}

func baseName(p string) string {
	for i := len(p) - 1; i >= 0; i-- {
		if p[i] == '/' || p[i] == '\\' {
			return p[i+1:]
		}
	}
	return p
}

package host

import (
	"context"
	"errors"
	"fmt"
	"io"
	"os"
	"strings"
	"sync"
	"time"

	"github.com/uniclipboard/unic/app/desktop/internal/api"
	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/clip"
)

// pathSource keeps a file where it is: the engine opens it again whenever it
// needs the bytes and compares size and modification time with what it saw.
type pathSource struct{ path string }

func (p pathSource) Open(context.Context) (io.ReadCloser, engine.FileMeta, error) {
	f, err := os.Open(p.path)
	if err != nil {
		return nil, engine.FileMeta{}, err
	}
	st, err := f.Stat()
	if err != nil || !st.Mode().IsRegular() {
		f.Close()
		return nil, engine.FileMeta{}, errors.New("not a regular file")
	}
	return f, engine.FileMeta{Size: st.Size(), Version: fmt.Sprint(st.ModTime().UnixNano())}, nil
}

// errUnrepresentable means a clipboard snapshot has nothing the engine can
// carry (a folder, a vanished file, only vendor formats).
var errUnrepresentable = errors.New("clipboard content cannot be synced")

// contentInput converts a clipboard snapshot. Only the formats that travel
// between devices are kept; files are referenced, not copied.
func contentInput(s clip.Snapshot) (engine.ContentInput, error) {
	var in engine.ContentInput
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
		st, err := os.Stat(f.Path)
		if err != nil || !st.Mode().IsRegular() {
			return in, errUnrepresentable // a folder or a vanished file: syncing its name as text would mislead
		}
		in.Files = append(in.Files, engine.FileInput{Name: f.Name, Size: st.Size(), Source: pathSource{f.Path}})
	}
	if len(in.Representations)+len(in.Files) == 0 {
		return in, errUnrepresentable
	}
	return in, nil
}

func bytesReader(b []byte) io.Reader { return strings.NewReader(string(b)) }

// describe says what kind of thing a snapshot is, for the event log. It never
// includes the content or file names.
func describe(s clip.Snapshot) string {
	switch {
	case len(s.Files) == 1:
		return "1 file"
	case len(s.Files) > 1:
		return fmt.Sprintf("%d files", len(s.Files))
	}
	for _, p := range s.Parts {
		if p.Format == clip.PNG {
			return "image"
		}
	}
	return "text"
}

// ---- event hub ----

type hub struct {
	mu   sync.Mutex
	subs map[chan api.Event]struct{}
}

func (h *hub) subscribe() (<-chan api.Event, func()) {
	ch := make(chan api.Event, 32)
	h.mu.Lock()
	if h.subs == nil {
		h.subs = map[chan api.Event]struct{}{}
	}
	h.subs[ch] = struct{}{}
	h.mu.Unlock()
	return ch, func() { h.mu.Lock(); delete(h.subs, ch); h.mu.Unlock() }
}

// emit never blocks: a slow subscriber misses events instead of stalling the daemon.
func (h *hub) emit(ev api.Event) {
	ev.Time = time.Now().Unix()
	h.mu.Lock()
	defer h.mu.Unlock()
	for ch := range h.subs {
		select {
		case ch <- ev:
		default:
		}
	}
}

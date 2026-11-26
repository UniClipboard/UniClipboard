package clipboardintegration

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"slices"
	"sync"

	"github.com/uniclipboard/unic/internal/clip"
	api "github.com/uniclipboard/unic/internal/localapi"
)

var (
	ErrPlain     = errors.New("clipboard: no plain-text representation")
	ErrClipboard = errors.New("clipboard changed")
)

// TrackedBoard records successful writes by the real IPC adapter. It does not
// own content retention and never writes outside the adapter protocol.
type TrackedBoard struct {
	board       Board
	mu          sync.Mutex
	serial      uint64
	mark        string
	generation  string
	fingerprint string
}

func NewTrackedBoard(b Board) *TrackedBoard          { return &TrackedBoard{board: b} }
func (b *TrackedBoard) Read() (clip.Snapshot, error) { return b.board.Read() }
func (b *TrackedBoard) ClipboardGeneration() string {
	if source, ok := b.board.(interface{ ClipboardGeneration() string }); ok {
		return source.ClipboardGeneration()
	}
	return ""
}

func (b *TrackedBoard) ClipboardChanges(ctx context.Context) <-chan struct{} {
	if source, ok := b.board.(interface {
		ClipboardChanges(context.Context) <-chan struct{}
	}); ok {
		return source.ClipboardChanges(ctx)
	}
	return nil
}

func (b *TrackedBoard) ReadFileImage(copyID string) []byte {
	if source, ok := b.board.(interface{ ReadFileImage(string) []byte }); ok {
		return source.ReadFileImage(copyID)
	}
	return nil
}
func (b *TrackedBoard) Write(s clip.Snapshot) error {
	if err := b.board.Write(s); err != nil {
		return err
	}
	b.mu.Lock()
	b.serial++
	b.generation = b.ClipboardGeneration()
	b.fingerprint = clipboardMark(s)
	b.mark = ""
	if b.fingerprint != "" {
		b.mark = fmt.Sprintf("%d:%s", b.serial, b.fingerprint)
	}
	b.mu.Unlock()
	return nil
}
func (b *TrackedBoard) Apply(ctx context.Context, client *api.Client, id string, plain bool) (string, error) {
	b.mu.Lock()
	before := b.serial
	b.mu.Unlock()
	requestBytes := [16]byte{}
	if _, err := rand.Read(requestBytes[:]); err != nil {
		return "", err
	}
	request := "quick-" + hex.EncodeToString(requestBytes[:])
	if plain {
		item, err := client.GetHistory(ctx, id)
		if err != nil {
			return "", err
		}
		hasPlain := false
		for _, part := range item.Parts {
			hasPlain = hasPlain || part.MIME == clip.Text
		}
		if !hasPlain {
			return "", ErrPlain
		}
		out, err := client.ExecuteAction(ctx, id, "builtin.text/plain", api.ActionInput{RequestID: request, Apply: true})
		if err != nil {
			return "", err
		}
		if !out.Applied || out.ApplyError != nil {
			return "", ErrClipboard
		}
	} else {
		if _, err := client.RestoreHistory(ctx, id, request); err != nil {
			return "", err
		}
	}
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.serial != before+1 || b.mark == "" {
		return "", ErrClipboard
	}
	return b.mark, nil
}
func (b *TrackedBoard) Matches(mark string) bool {
	if mark == "" {
		return false
	}
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.mark != mark {
		return false
	}
	if b.generation != "" {
		if b.ClipboardGeneration() != b.generation {
			return false
		}
		if probe, ok := b.board.(interface{ ClipboardAvailable() bool }); ok && !probe.ClipboardAvailable() {
			return false
		}
		return b.ClipboardGeneration() == b.generation
	}
	s, err := b.Read()
	return err == nil && clipboardMark(s) == b.fingerprint
}

// Clipboard ownership also includes file paths. clip.Hash intentionally excludes
// paths for sync deduplication; paste must detect a same-name file replacement.
func clipboardMark(s clip.Snapshot) string {
	if s.Empty() {
		return ""
	}
	h := sha256.New()
	_, _ = fmt.Fprint(h, s.Hash())
	files := slices.Clone(s.Files)
	slices.SortFunc(files, func(a, b clip.File) int {
		if a.Path < b.Path {
			return -1
		}
		if a.Path > b.Path {
			return 1
		}
		return 0
	})
	for _, f := range files {
		_, _ = fmt.Fprintf(h, "%d:%s", len(f.Path), f.Path)
	}
	return hex.EncodeToString(h.Sum(nil))
}

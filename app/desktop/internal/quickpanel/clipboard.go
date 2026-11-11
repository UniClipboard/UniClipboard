package quickpanel

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"slices"
	"sync"

	"github.com/uniclipboard/unic/internal/clip"
	api "github.com/uniclipboard/unic/internal/localapi"
)

type Board interface {
	Read() (clip.Snapshot, error)
	Write(clip.Snapshot) error
}

// Clipboard records successful writes by the real IPC adapter. It does not
// own content retention and never writes outside the adapter protocol.
type Clipboard struct {
	board  Board
	mu     sync.Mutex
	serial uint64
	mark   string
}

func NewClipboard(b Board) *Clipboard             { return &Clipboard{board: b} }
func (b *Clipboard) Read() (clip.Snapshot, error) { return b.board.Read() }
func (b *Clipboard) Write(s clip.Snapshot) error {
	if err := b.board.Write(s); err != nil {
		return err
	}
	b.mu.Lock()
	b.serial++
	b.mark = clipboardMark(s)
	b.mu.Unlock()
	return nil
}
func (b *Clipboard) Apply(ctx context.Context, client *api.Client, id string, plain bool) (string, error) {
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
func (b *Clipboard) Matches(mark string) bool {
	if mark == "" {
		return false
	}
	s, err := b.Read()
	return err == nil && clipboardMark(s) == mark
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

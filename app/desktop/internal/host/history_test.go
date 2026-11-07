package host

import (
	"io"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/uniclipboard/unic/app/desktop/internal/api"
	"github.com/uniclipboard/unic/internal/clip"
)

func historyIDs(t *testing.T, h *host) []api.HistoryItem {
	t.Helper()
	p, err := h.n.ListHistory(bg, api.HistoryQuery{})
	if err != nil {
		t.Fatal(err)
	}
	return p.Items
}

func TestCopiesLandInHistoryAndCanBeUsedAgain(t *testing.T) {
	h := newHost(t, false)
	settle(h)
	h.board.user(text("first"))
	eventually(t, "first recorded", func() bool { return len(historyIDs(t, h)) == 1 })
	h.board.user(text("second"))
	eventually(t, "second recorded", func() bool { return len(historyIDs(t, h)) == 2 })
	items := historyIDs(t, h)
	if items[0].Kind != "text" || items[0].Origin != "local_system" || items[0].TotalBytes != 6 {
		t.Fatalf("newest = %+v", items[0])
	}
	// Content is readable on request.
	c, err := h.n.HistoryContent(bg, items[1].ID, 0)
	if err != nil {
		t.Fatal(err)
	}
	b, _ := io.ReadAll(c)
	_ = c.Close()
	if string(b) != "first" || c.MIME != "text/plain" {
		t.Fatalf("content %q %q", b, c.MIME)
	}
	if _, err := h.n.HistoryContent(bg, items[1].ID, 5); err == nil {
		t.Fatal("a missing part must fail")
	}
	// Use the older one: the clipboard shows it, and it is not recorded as a new copy.
	if err := h.n.UseHistory(bg, items[1].ID); err != nil {
		t.Fatal(err)
	}
	if got(h) != "first" {
		t.Fatalf("clipboard = %q", got(h))
	}
	time.Sleep(150 * time.Millisecond) // several polls
	if n := len(historyIDs(t, h)); n != 2 {
		t.Fatalf("using a record added history: %d items", n)
	}
	// Tags and delete use the revision.
	tags := []string{"keep"}
	up, err := h.n.UpdateHistory(bg, items[1].ID, api.HistoryUpdate{Revision: items[1].Revision, Tags: &tags})
	if err != nil || up.Revision != items[1].Revision+1 || len(up.Tags) != 1 {
		t.Fatalf("%+v %v", up, err)
	}
	if _, err := h.n.UpdateHistory(bg, items[1].ID, api.HistoryUpdate{Revision: items[1].Revision, Tags: &tags}); apiStatus(err) != 409 {
		t.Fatalf("stale revision: %v", err)
	}
	if err := h.n.DeleteHistory(bg, items[1].ID, up.Revision); err != nil {
		t.Fatal(err)
	}
	if _, err := h.n.GetHistory(bg, items[1].ID); apiStatus(err) != 404 {
		t.Fatalf("deleted: %v", err)
	}
}

func apiStatus(err error) int {
	if e, ok := err.(*api.Error); ok {
		return e.Status
	}
	return 0
}

func TestCopiedFileIsRememberedByPathNotCopied(t *testing.T) {
	h := newHost(t, false)
	settle(h)
	src := filepath.Join(t.TempDir(), "report.txt")
	_ = os.WriteFile(src, []byte("hello file"), 0o644)
	h.board.user(clip.Snapshot{Files: []clip.File{{Name: "report.txt", Path: src, Size: 10}}})
	eventually(t, "recorded", func() bool { return len(historyIDs(t, h)) == 1 })
	it := historyIDs(t, h)[0]
	if it.Kind != "files" || it.TotalBytes != 10 {
		t.Fatalf("%+v", it)
	}
	if ents, _ := os.ReadDir(filepath.Join(h.cfgDir(), "history")); len(ents) != 0 {
		t.Fatalf("history copied the file: %d entries", len(ents))
	}
	h.board.user(text("something else"))
	eventually(t, "second recorded", func() bool { return len(historyIDs(t, h)) == 2 })
	if err := h.n.UseHistory(bg, it.ID); err != nil {
		t.Fatal(err)
	}
	s, _ := h.board.Read()
	if len(s.Files) != 1 || s.Files[0].Path != src {
		t.Fatalf("clipboard files = %+v (want the original path)", s.Files)
	}
	// The file moved away: using it again fails cleanly instead of pasting nothing.
	_ = os.Remove(src)
	h.board.user(text("again"))
	eventually(t, "third recorded", func() bool { return len(historyIDs(t, h)) == 3 })
	if err := h.n.UseHistory(bg, it.ID); err == nil {
		t.Fatal("using a file that is gone must fail")
	}
}

// settle lets the host take what is on the clipboard now as its baseline; that
// first look is not a copy and is not recorded.
func settle(h *host) {
	h.board.user(text("startup"))
	time.Sleep(150 * time.Millisecond)
}

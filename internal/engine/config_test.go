package engine

import (
	"context"
	"os"
	"path/filepath"
	"testing"
	"time"

	"unic/internal/config"
	"unic/internal/store"
)

// newConfigured is a node whose settings come from a real config file; edit
// returns a function that saves new content and reloads it.
func newConfigured(t *testing.T, id string) (node, func(string)) {
	t.Helper()
	path := filepath.Join(t.TempDir(), "config.toml")
	m, err := config.Open(path, t.Logf)
	if err != nil {
		t.Fatal(err)
	}
	st, err := store.Open(filepath.Join(t.TempDir(), "db"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = st.Close() })
	b := &fakeBoard{}
	e := New(Config{ID: id, Name: id, Board: b, Store: st, Transport: newTCP(t),
		Inbox: t.TempDir(), Poll: 10 * time.Millisecond, Logf: t.Logf, Settings: m})
	ctx, cancel := context.WithCancel(context.Background())
	t.Cleanup(cancel)
	go func() { _ = e.Run(ctx) }()
	return node{e, b}, func(content string) {
		t.Helper()
		if err := os.WriteFile(path, []byte(content), 0o600); err != nil {
			t.Fatal(err)
		}
		if err := m.Reload(); err != nil {
			t.Fatal(err)
		}
	}
}

func TestPausedSyncKeepsConnection(t *testing.T) {
	a, _ := newConfigured(t, "aaa")
	b, editB := newConfigured(t, "bbb")
	pair(t, a, b)

	editB("[sync]\nenabled = false")
	a.board.user(text("while paused"))
	time.Sleep(300 * time.Millisecond)
	if got(b) != "" {
		t.Fatalf("paused device applied a clip: %q", got(b))
	}
	if !connected(a, b) {
		t.Fatal("pausing dropped the connection")
	}

	editB("[sync]\nenabled = true")
	a.board.user(text("after resume"))
	eventually(t, "b receives after resume", func() bool { return got(b) == "after resume" })
}

func TestSyncTypeSwitches(t *testing.T) {
	a, editA := newConfigured(t, "aaa")
	b, _ := newConfigured(t, "bbb")
	pair(t, a, b)

	editA("[sync]\ntext = false")
	a.board.user(text("secret"))
	time.Sleep(300 * time.Millisecond)
	if got(b) != "" {
		t.Fatalf("text was sent although sync.text = false: %q", got(b))
	}
	editA("")
	a.board.user(text("public"))
	eventually(t, "text flows again", func() bool { return got(b) == "public" })
}

func TestReceiveLimitDropsClipButKeepsStreamInSync(t *testing.T) {
	a, _ := newConfigured(t, "aaa")
	b, editB := newConfigured(t, "bbb")
	pair(t, a, b)

	editB("[limits]\nmax_total_bytes = 100")
	big := make([]byte, 5000)
	for i := range big {
		big[i] = 'x'
	}
	a.board.user(text(string(big)))
	time.Sleep(300 * time.Millisecond)
	if got(b) != "" {
		t.Fatal("oversize clip was applied")
	}
	// The blob was consumed, not left in the stream: the next clip still works.
	a.board.user(text("small"))
	eventually(t, "small clip after dropped one", func() bool { return got(b) == "small" })
	if !connected(a, b) {
		t.Fatal("the connection did not survive an oversize clip")
	}
}

func TestSendLimit(t *testing.T) {
	a, editA := newConfigured(t, "aaa")
	b, _ := newConfigured(t, "bbb")
	pair(t, a, b)
	editA("[limits]\nmax_total_bytes = 10")
	a.board.user(text("this is longer than ten bytes"))
	time.Sleep(300 * time.Millisecond)
	if got(b) != "" {
		t.Fatal("sent a clip over the sender's own limit")
	}
}

func TestDeviceNameFollowsConfig(t *testing.T) {
	a, editA := newConfigured(t, "aaa")
	if _, name, _ := a.e.Self(); name != "aaa" {
		t.Fatalf("default name = %q", name)
	}
	editA(`device_name = "Work Mac"`)
	if _, name, _ := a.e.Self(); name != "Work Mac" {
		t.Fatalf("name after edit = %q", name)
	}
	editA("")
	if _, name, _ := a.e.Self(); name != "aaa" {
		t.Fatalf("name after removing the key = %q", name)
	}
}

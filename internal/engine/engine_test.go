package engine

import (
	"context"
	"net"
	"os"
	"path/filepath"
	"sync"
	"testing"
	"time"

	"unic/internal/clip"
	"unic/internal/store"
)

// fakeBoard is an in-memory clipboard. extra mimics systems that add their own
// representations to whatever is written.
type fakeBoard struct {
	mu    sync.Mutex
	s     clip.Snapshot
	extra bool
	wrote int
}

func (b *fakeBoard) Read() (clip.Snapshot, error) {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.s, nil
}

func (b *fakeBoard) Write(s clip.Snapshot) error {
	b.mu.Lock()
	defer b.mu.Unlock()
	b.wrote++
	b.s = clip.Snapshot{Parts: append([]clip.Part(nil), s.Parts...), Files: s.Files}
	if b.extra {
		b.s.Parts = append(b.s.Parts, clip.Part{Format: "x/system-added", Data: []byte("1")})
	}
	return nil
}

func (b *fakeBoard) user(s clip.Snapshot) { b.mu.Lock(); b.s = s; b.mu.Unlock() }
func (b *fakeBoard) writes() int          { b.mu.Lock(); defer b.mu.Unlock(); return b.wrote }

type tcpTransport struct{ ln net.Listener }

func newTCP(t *testing.T) *tcpTransport {
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { ln.Close() })
	return &tcpTransport{ln}
}
func (t *tcpTransport) Listen(context.Context) (net.Listener, error) { return t.ln, nil }
func (t *tcpTransport) Dial(ctx context.Context, a string) (net.Conn, error) {
	var d net.Dialer
	return d.DialContext(ctx, "tcp", a)
}
func (t *tcpTransport) Addr() string { return t.ln.Addr().String() }

type node struct {
	e     *Engine
	board *fakeBoard
}

func newNode(t *testing.T, id string, extra bool) node {
	t.Helper()
	st, err := store.Open(filepath.Join(t.TempDir(), "db"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { st.Close() })
	b := &fakeBoard{extra: extra}
	e := New(Config{ID: id, Name: id, Board: b, Store: st, Transport: newTCP(t),
		Inbox: t.TempDir(), Poll: 10 * time.Millisecond, Logf: t.Logf})
	ctx, cancel := context.WithCancel(context.Background())
	t.Cleanup(cancel)
	go e.Run(ctx)
	return node{e, b}
}

func text(s string) clip.Snapshot {
	return clip.Snapshot{Parts: []clip.Part{{Format: clip.Text, Data: []byte(s)}}}
}

func eventually(t *testing.T, what string, f func() bool) {
	t.Helper()
	for i := 0; i < 300; i++ {
		if f() {
			return
		}
		time.Sleep(10 * time.Millisecond)
	}
	t.Fatalf("timed out waiting for %s", what)
}

func got(n node) string {
	s, _ := n.board.Read()
	for _, p := range s.Parts {
		if p.Format == clip.Text {
			return string(p.Data)
		}
	}
	return ""
}

func pair(t *testing.T, a, b node) {
	t.Helper()
	_, _, addr := a.e.Self()
	time.Sleep(20 * time.Millisecond) // let Run install its context
	if _, err := b.e.AddPeer(context.Background(), addr); err != nil {
		t.Fatal(err)
	}
	eventually(t, "both sides connected", func() bool {
		pa, _ := a.e.Peers()
		pb, _ := b.e.Peers()
		return len(pa) == 1 && pa[0].Connected && len(pb) == 1 && pb[0].Connected
	})
}

func TestSyncBothWaysWithoutEcho(t *testing.T) {
	a, b := newNode(t, "aaa", true), newNode(t, "bbb", true) // both add system formats
	pair(t, a, b)

	a.board.user(text("from a"))
	eventually(t, "b receives", func() bool { return got(b) == "from a" })
	b.board.user(text("from b"))
	eventually(t, "a receives", func() bool { return got(a) == "from b" })

	// No ping-pong: each side applied exactly one remote change.
	time.Sleep(200 * time.Millisecond)
	if a.board.writes() != 1 || b.board.writes() != 1 {
		t.Fatalf("echo detected: writes a=%d b=%d", a.board.writes(), b.board.writes())
	}
}

func TestPairingIsMutualAndRemembered(t *testing.T) {
	a, b := newNode(t, "aaa", false), newNode(t, "bbb", false)
	pair(t, a, b)
	// a never dialed b, yet it knows b for reconnecting after a restart.
	ps, err := a.e.cfg.Store.Peers()
	if err != nil || len(ps) != 1 || ps[0].ID != "bbb" || ps[0].Addr == "" {
		t.Fatalf("a's stored peers = %+v, %v", ps, err)
	}
}

func TestFileSync(t *testing.T) {
	a, b := newNode(t, "aaa", false), newNode(t, "bbb", false)
	pair(t, a, b)
	src := filepath.Join(t.TempDir(), "report.txt")
	os.WriteFile(src, []byte("hello file"), 0o644)
	a.board.user(clip.Snapshot{Files: []clip.File{{Name: "report.txt", Path: src, Size: 10}}})
	eventually(t, "file arrives", func() bool {
		s, _ := b.board.Read()
		return len(s.Files) == 1
	})
	s, _ := b.board.Read()
	data, err := os.ReadFile(s.Files[0].Path)
	if err != nil || string(data) != "hello file" || s.Files[0].Name != "report.txt" {
		t.Fatalf("got %q %v %+v", data, err, s.Files)
	}
}

func TestImageAndRichText(t *testing.T) {
	a, b := newNode(t, "aaa", false), newNode(t, "bbb", false)
	pair(t, a, b)
	png := append([]byte("\x89PNG\r\n\x1a\n"), make([]byte, 1<<20)...)
	a.board.user(clip.Snapshot{Parts: []clip.Part{
		{Format: clip.Text, Data: []byte("hi")},
		{Format: clip.HTML, Data: []byte("<b>hi</b>")},
		{Format: clip.PNG, Data: png},
	}})
	eventually(t, "rich clip arrives", func() bool { s, _ := b.board.Read(); return len(s.Parts) == 3 })
	s, _ := b.board.Read()
	for _, p := range s.Parts {
		if p.Format == clip.PNG && len(p.Data) != len(png) {
			t.Fatalf("png truncated: %d", len(p.Data))
		}
	}
}

func TestSimultaneousDialYieldsOneSession(t *testing.T) {
	a, b := newNode(t, "aaa", false), newNode(t, "bbb", false)
	time.Sleep(20 * time.Millisecond)
	_, _, aa := a.e.Self()
	_, _, ba := b.e.Self()
	go a.e.AddPeer(context.Background(), ba)
	go b.e.AddPeer(context.Background(), aa)
	eventually(t, "connected", func() bool {
		pa, _ := a.e.Peers()
		pb, _ := b.e.Peers()
		return len(pa) == 1 && pa[0].Connected && len(pb) == 1 && pb[0].Connected
	})
	a.board.user(text("still works"))
	eventually(t, "sync after race", func() bool { return got(b) == "still works" })
}

func TestStartupClipboardIsNotBroadcast(t *testing.T) {
	a, b := newNode(t, "aaa", false), newNode(t, "bbb", false)
	a.board.user(text("old secret"))
	pair(t, a, b)
	time.Sleep(150 * time.Millisecond)
	if got(b) != "" {
		t.Fatalf("pre-existing clipboard leaked to peer: %q", got(b))
	}
}

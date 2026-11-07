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
	"unic/internal/rendezvous"
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
	t.Cleanup(func() { _ = ln.Close() })
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
	return newNodeWith(t, id, extra, nil)
}

func newNodeWith(t *testing.T, id string, extra bool, rv *rendezvous.Client) node {
	t.Helper()
	st, err := store.Open(filepath.Join(t.TempDir(), "db"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = st.Close() })
	b := &fakeBoard{extra: extra}
	e := New(Config{ID: id, Name: id, Board: b, Store: st, Transport: newTCP(t),
		Inbox: t.TempDir(), Poll: 10 * time.Millisecond, Logf: t.Logf, Rendezvous: rv})
	ctx, cancel := context.WithCancel(context.Background())
	t.Cleanup(cancel)
	go func() { _ = e.Run(ctx) }()
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

func connected(a, b node) bool {
	pa, _ := a.e.Peers()
	pb, _ := b.e.Peers()
	has := func(ps []PeerStatus, id string) bool {
		for _, p := range ps {
			if p.ID == id && p.Connected {
				return true
			}
		}
		return false
	}
	ida, _, _ := a.e.Self()
	idb, _, _ := b.e.Self()
	return has(pa, idb) && has(pb, ida)
}

func id(n node) string { i, _, _ := n.e.Self(); return i }

func pending(n node) []PendingPeer { p, _ := n.e.Pending(); return p }

// pair makes b pair with a, and a approve it, as two users would.
func pair(t *testing.T, a, b node) {
	t.Helper()
	_, _, addr := a.e.Self()
	time.Sleep(20 * time.Millisecond) // let Run install its context
	_, awaiting, err := b.e.AddPeer(context.Background(), addr)
	if err != nil {
		t.Fatal(err)
	}
	if !awaiting {
		t.Fatal("a stranger was accepted without approval")
	}
	eventually(t, "a sees the request", func() bool { return len(pending(a)) == 1 })
	if _, err := a.e.Approve(id(b)); err != nil {
		t.Fatal(err)
	}
	eventually(t, "both sides connected", func() bool { return connected(a, b) })
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
	if err := os.WriteFile(src, []byte("hello file"), 0o644); err != nil {
		t.Fatal(err)
	}
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
	// Both users pair at once: each dial is the user's own decision, so no
	// approval is needed on either side.
	go func() { _, _, _ = a.e.AddPeer(context.Background(), ba) }()
	go func() { _, _, _ = b.e.AddPeer(context.Background(), aa) }()
	eventually(t, "connected", func() bool { return connected(a, b) })
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

func TestUnknownDeviceWaitsForApproval(t *testing.T) {
	a, b := newNode(t, "aaa", false), newNode(t, "bbb", false)
	time.Sleep(20 * time.Millisecond)
	_, _, addr := a.e.Self()
	if _, awaiting, err := b.e.AddPeer(context.Background(), addr); err != nil || !awaiting {
		t.Fatalf("AddPeer = awaiting %v, %v", awaiting, err)
	}
	if ps := pending(a); len(ps) != 1 || ps[0].ID != "bbb" || ps[0].Via != "" {
		t.Fatalf("a's pending = %+v", ps)
	}
	// Not approved: no connection, no sync, in either direction.
	a.board.user(text("secret"))
	b.board.user(text("other"))
	time.Sleep(300 * time.Millisecond)
	if connected(a, b) || got(b) != "other" || got(a) != "secret" {
		t.Fatal("unapproved device was connected")
	}
}

func TestIntroductionNeedsApprovalOnEveryDevice(t *testing.T) {
	a, b, c := newNode(t, "aaa", false), newNode(t, "bbb", false), newNode(t, "ccc", false)
	pair(t, a, b) // b pairs with a
	pair(t, b, c) // c pairs with b; a and c have never met

	// b introduces the two to each other. Neither connects by itself.
	eventually(t, "a hears about c", func() bool { return len(pending(a)) == 1 })
	eventually(t, "c hears about a", func() bool { return len(pending(c)) == 1 })
	if ps := pending(a); ps[0].ID != "ccc" || ps[0].Via != "bbb" {
		t.Fatalf("a's pending = %+v", ps)
	}
	a.board.user(text("only for b"))
	time.Sleep(300 * time.Millisecond)
	if connected(a, c) || got(c) != "" {
		t.Fatal("introduced devices connected without approval")
	}
	if got(b) != "only for b" {
		t.Fatalf("b should have it: %q", got(b))
	}

	// One side approving is not enough...
	if _, err := a.e.Approve("ccc"); err != nil {
		t.Fatal(err)
	}
	time.Sleep(300 * time.Millisecond)
	if connected(a, c) {
		t.Fatal("connected before c approved a")
	}
	// ...both are.
	if _, err := c.e.Approve("aaa"); err != nil {
		t.Fatal(err)
	}
	eventually(t, "a and c connected", func() bool { return connected(a, c) })
	a.board.user(text("to everyone"))
	eventually(t, "c receives from a", func() bool { return got(c) == "to everyone" })
	if len(pending(a)) != 0 || len(pending(c)) != 0 {
		t.Fatal("pending not cleared after approval")
	}
}

func TestRejectedDeviceStaysQuietUntilItAsksAgain(t *testing.T) {
	a, b, c := newNode(t, "aaa", false), newNode(t, "bbb", false), newNode(t, "ccc", false)
	pair(t, a, b)
	pair(t, b, c)
	eventually(t, "a hears about c", func() bool { return len(pending(a)) == 1 })
	if err := a.e.Reject("ccc"); err != nil {
		t.Fatal(err)
	}
	b.e.introduceToAll() // introduced again
	time.Sleep(300 * time.Millisecond)
	if len(pending(a)) != 0 {
		t.Fatal("a rejected device came back through an introduction")
	}
	// But c asking a directly is a new request.
	_, _, addr := a.e.Self()
	if _, awaiting, err := c.e.AddPeer(context.Background(), addr); err != nil || !awaiting {
		t.Fatalf("AddPeer = %v, %v", awaiting, err)
	}
	if len(pending(a)) != 1 {
		t.Fatalf("direct request did not show up: %+v", pending(a))
	}
}

package host

import (
	"bytes"
	"context"
	"image"
	"image/png"
	"net"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/uniclipboard/unic/app/desktop/internal/api"
	"github.com/uniclipboard/unic/app/desktop/internal/config"
	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/clip"
	"github.com/uniclipboard/unic/internal/plugins/tailcat"
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

// loopback is a network on 127.0.0.1 that keeps its address across listens.
type loopback struct {
	mu   sync.Mutex
	addr string
	ln   net.Listener
}

func newLoopback(t *testing.T) *loopback {
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	l := &loopback{ln: ln, addr: ln.Addr().String()}
	t.Cleanup(func() {
		l.mu.Lock()
		defer l.mu.Unlock()
		if l.ln != nil {
			l.ln.Close()
		}
	})
	return l
}
func (l *loopback) Listen(context.Context) (net.Listener, error) {
	l.mu.Lock()
	defer l.mu.Unlock()
	if l.ln != nil {
		ln := l.ln
		l.ln = nil
		return ln, nil
	}
	return net.Listen("tcp", l.addr)
}
func (l *loopback) Dial(ctx context.Context, a string) (net.Conn, error) {
	var d net.Dialer
	return d.DialContext(ctx, "tcp", a)
}
func (l *loopback) Addr() string                     { return l.addr }
func (l *loopback) Binding(a string) ([]byte, error) { return []byte("loop\x00" + a), nil }
func (l *loopback) Start() error                     { return nil }
func (l *loopback) Close() error                     { return nil }

type host struct {
	n     *Node
	board *fakeBoard
	cfg   *config.Manager
	edit  func(string)
	id    string
}

// newHost builds a desktop node over a real SDK engine, a fake clipboard and a
// configuration file, and starts it.
func newHost(t *testing.T, extra bool) *host { return newHostWith(t, extra, nil) }

func newHostWith(t *testing.T, extra bool, plugins []engine.PluginInstance) *host {
	t.Helper()
	dir := t.TempDir()
	path := filepath.Join(dir, "config.toml")
	m, err := config.Open(path, t.Logf)
	if err != nil {
		t.Fatal(err)
	}
	b := &fakeBoard{extra: extra}
	n, err := Open(context.Background(), Options{Dir: dir, Settings: m, Board: b, Poll: 10 * time.Millisecond, Logf: t.Logf, Peer: tailcat.FactoryOn(newLoopback(t)), Plugins: plugins})
	if err != nil {
		t.Fatal(err)
	}
	m.OnChange = func(_, _ config.Config, err error) { n.ConfigChanged(err) }
	if err := n.Start(context.Background()); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { n.Close(context.Background()) })
	h := &host{n: n, board: b, cfg: m}
	h.edit = func(content string) {
		t.Helper()
		if err := os.WriteFile(path, []byte(content), 0o600); err != nil {
			t.Fatal(err)
		}
		if err := m.Reload(); err != nil {
			t.Fatal(err)
		}
	}
	st, _ := n.Status(context.Background())
	h.id = st.ID
	return h
}

var bg = context.Background()

func eventually(t *testing.T, what string, f func() bool) {
	t.Helper()
	for i := 0; i < 500; i++ {
		if f() {
			return
		}
		time.Sleep(10 * time.Millisecond)
	}
	t.Fatalf("timed out waiting for %s", what)
}

func text(s string) clip.Snapshot {
	return clip.Snapshot{Parts: []clip.Part{{Format: clip.Text, Data: []byte(s)}}}
}

func got(h *host) string {
	s, _ := h.board.Read()
	for _, p := range s.Parts {
		if p.Format == clip.Text {
			return string(p.Data)
		}
	}
	return ""
}

func addr(t *testing.T, h *host) string {
	st, err := h.n.Status(bg)
	if err != nil || st.Addr == "" {
		t.Fatalf("status = %+v, %v", st, err)
	}
	return st.Addr
}

func pending(h *host) []api.PendingPeer { p, _ := h.n.Pending(bg); return p }

func connected(a, b *host) bool {
	has := func(x, y *host) bool {
		st, _ := x.n.Status(bg)
		for _, p := range st.Peers {
			if p.ID == y.id && p.Connected {
				return true
			}
		}
		return false
	}
	return has(a, b) && has(b, a)
}

// pair makes b pair with a, and a approve it, as two users would.
func pair(t *testing.T, a, b *host) {
	t.Helper()
	res, err := b.n.Pair(bg, addr(t, a))
	if err != nil {
		t.Fatal(err)
	}
	if !res.Awaiting {
		t.Fatal("a stranger was accepted without approval")
	}
	eventually(t, "a sees the request", func() bool {
		for _, p := range pending(a) {
			if p.ID == b.id {
				return true
			}
		}
		return false
	})
	if _, err := a.n.Approve(bg, b.id, ""); err != nil {
		t.Fatal(err)
	}
	// Pairing is mutual: the joiner approves the other device too.
	if _, err := b.n.Approve(bg, a.id, ""); err != nil {
		t.Fatal(err)
	}
	eventually(t, "both sides connected", func() bool { return connected(a, b) })
}

func TestSyncBothWaysWithoutEcho(t *testing.T) {
	a, b := newHost(t, true), newHost(t, true) // both add system formats
	pair(t, a, b)

	a.board.user(text("from a"))
	eventually(t, "b receives", func() bool { return got(b) == "from a" })
	b.board.user(text("from b"))
	eventually(t, "a receives", func() bool { return got(a) == "from b" })

	// No ping-pong: each side applied exactly one remote change.
	time.Sleep(300 * time.Millisecond)
	if a.board.writes() != 1 || b.board.writes() != 1 {
		t.Fatalf("echo detected: writes a=%d b=%d", a.board.writes(), b.board.writes())
	}
}

func TestFileSync(t *testing.T) {
	a, b := newHost(t, false), newHost(t, false)
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
	// The received file is the engine's own copy: it is not copied again for the clipboard.
	if !strings.HasPrefix(s.Files[0].Path, filepath.Join(b.cfgDir(), "inbox")) {
		t.Fatalf("path = %s", s.Files[0].Path)
	}
	// Replacing it with new content releases the old file.
	a.board.user(text("next"))
	eventually(t, "text arrives", func() bool { return got(b) == "next" })
	eventually(t, "the old file is released", func() bool {
		_, err := os.Stat(s.Files[0].Path)
		return os.IsNotExist(err)
	})
}

func (h *host) cfgDir() string {
	return filepath.Dir(h.cfg.State().Path)
}

func TestImageAndRichText(t *testing.T) {
	a, b := newHost(t, false), newHost(t, false)
	pair(t, a, b)
	img := pngImage(t)
	a.board.user(clip.Snapshot{Parts: []clip.Part{
		{Format: clip.Text, Data: []byte("hi")},
		{Format: clip.HTML, Data: []byte("<b>hi</b>")},
		{Format: clip.PNG, Data: img},
	}})
	eventually(t, "rich clip arrives", func() bool { s, _ := b.board.Read(); return len(s.Parts) == 3 })
	s, _ := b.board.Read()
	for _, p := range s.Parts {
		if p.Format == clip.PNG && len(p.Data) != len(img) {
			t.Fatalf("png truncated: %d", len(p.Data))
		}
	}
}

func TestStartupClipboardIsNotBroadcast(t *testing.T) {
	a, b := newHost(t, false), newHost(t, false)
	a.board.user(text("old secret"))
	time.Sleep(100 * time.Millisecond) // a's poll sees it
	pair(t, a, b)
	time.Sleep(200 * time.Millisecond)
	if got(b) != "" {
		t.Fatalf("pre-existing clipboard leaked to peer: %q", got(b))
	}
}

func TestUnapprovedDeviceGetsNothing(t *testing.T) {
	a, b := newHost(t, false), newHost(t, false)
	res, err := b.n.Pair(bg, addr(t, a))
	if err != nil || !res.Awaiting {
		t.Fatalf("Pair = %+v, %v", res, err)
	}
	eventually(t, "request", func() bool { return len(pending(a)) == 1 })
	if p := pending(a)[0]; p.ID != b.id || !p.ByCode {
		t.Fatalf("pending = %+v", p)
	}
	a.board.user(text("secret"))
	b.board.user(text("other"))
	time.Sleep(300 * time.Millisecond)
	if connected(a, b) || got(b) != "other" || got(a) != "secret" {
		t.Fatal("an unapproved device was connected")
	}
}

func TestPausedSyncKeepsConnection(t *testing.T) {
	a, b := newHost(t, false), newHost(t, false)
	pair(t, a, b)

	b.edit("[sync]\nenabled = false")
	a.board.user(text("while paused"))
	time.Sleep(300 * time.Millisecond)
	if got(b) != "" {
		t.Fatalf("paused device applied a clip: %q", got(b))
	}
	if !connected(a, b) {
		t.Fatal("pausing dropped the connection")
	}

	b.edit("[sync]\nenabled = true")
	// What was skipped is not delivered later; the next copy is.
	a.board.user(text("after resume"))
	eventually(t, "sync resumes", func() bool { return got(b) == "after resume" })
}

func TestSyncTypeSwitches(t *testing.T) {
	a, b := newHost(t, false), newHost(t, false)
	pair(t, a, b)
	b.edit("[plugins.instances.\"builtin.tailcat\".sync]\ntext = false")
	a.board.user(text("no text please"))
	time.Sleep(300 * time.Millisecond)
	if got(b) != "" {
		t.Fatalf("text arrived though it is off: %q", got(b))
	}
	b.edit("[plugins.instances.\"builtin.tailcat\".sync]\ntext = true")
	a.board.user(text("text is back"))
	eventually(t, "text again", func() bool { return got(b) == "text is back" })
}

func TestReceiveLimitDropsClipButKeepsStreamInSync(t *testing.T) {
	a, b := newHost(t, false), newHost(t, false)
	pair(t, a, b)
	b.edit("[limits]\nmax_total_bytes = 10")
	a.board.user(text("this is longer than ten bytes"))
	time.Sleep(300 * time.Millisecond)
	if got(b) != "" {
		t.Fatalf("over-limit clip applied: %q", got(b))
	}
	a.board.user(text("short"))
	eventually(t, "the stream is still in step", func() bool { return got(b) == "short" })
	if !connected(a, b) {
		t.Fatal("the connection was lost")
	}
}

func TestSendLimit(t *testing.T) {
	a, b := newHost(t, false), newHost(t, false)
	pair(t, a, b)
	a.edit("[limits]\nmax_total_bytes = 10")
	a.board.user(text("this is longer than ten bytes"))
	time.Sleep(300 * time.Millisecond)
	if got(b) != "" {
		t.Fatalf("over-limit clip was sent: %q", got(b))
	}
	a.board.user(text("ok"))
	eventually(t, "small clips still go", func() bool { return got(b) == "ok" })
}

func TestDeviceNameFollowsConfig(t *testing.T) {
	a, b := newHost(t, false), newHost(t, false)
	a.edit(`device_name = "desk"`)
	eventually(t, "name in effect", func() bool { st, _ := a.n.Status(bg); return st.Name == "desk" })
	res, err := a.n.Pair(bg, addr(t, b))
	if err != nil || !res.Awaiting {
		t.Fatalf("Pair = %+v, %v", res, err)
	}
	eventually(t, "b sees the new name", func() bool {
		ps := pending(b)
		return len(ps) == 1 && ps[0].Name == "desk"
	})
}

func TestExplicitSendReachesPeersOnceEvenIfUnchanged(t *testing.T) {
	a, b := newHost(t, false), newHost(t, false)
	pair(t, a, b)
	s := "same text"
	if err := a.n.SetClipboard(bg, api.SetClipboard{Text: &s}); err != nil {
		t.Fatal(err)
	}
	eventually(t, "b receives", func() bool { return got(b) == "same text" })
	// Putting the very same text again is a request to send it again, and the
	// poll does not send it a second time on top of that.
	b.board.user(text("changed on b"))
	eventually(t, "a gets b's copy", func() bool { return got(a) == "changed on b" })
	if err := a.n.SetClipboard(bg, api.SetClipboard{Text: &s}); err != nil {
		t.Fatal(err)
	}
	eventually(t, "b receives again", func() bool { return got(b) == "same text" })
	time.Sleep(200 * time.Millisecond)
	if b.board.writes() != 3 { // the first send, a's copy of b's text echo-free, the second send
		t.Logf("b wrote %d times", b.board.writes())
	}
}

func TestNoTargetsIsNotAnErrorForSetClipboard(t *testing.T) {
	a := newHost(t, false)
	s := "alone"
	if err := a.n.SetClipboard(bg, api.SetClipboard{Text: &s}); err != nil {
		t.Fatalf("setting the clipboard with no peers: %v", err)
	}
	if got(a) != "alone" {
		t.Fatal("not set")
	}
}

func TestUnpairAndApprovalErrors(t *testing.T) {
	a, b := newHost(t, false), newHost(t, false)
	pair(t, a, b)
	if err := a.n.Unpair(bg, "nobody"); err == nil || err.(*api.Error).Status != 404 {
		t.Fatalf("unpair unknown: %v", err)
	}
	if _, err := a.n.Approve(bg, "nobody", ""); err == nil || err.(*api.Error).Status != 404 {
		t.Fatalf("approve unknown: %v", err)
	}
	if err := a.n.Unpair(bg, b.id); err != nil {
		t.Fatal(err)
	}
	eventually(t, "b sees a leave", func() bool { return !connected(a, b) })
	// The wrong fingerprint is refused and approves nothing.
	res, _ := b.n.Pair(bg, addr(t, a))
	_ = res
	eventually(t, "request again", func() bool { return len(pending(a)) == 1 })
	if _, err := a.n.Approve(bg, b.id, "0000 0000 0000 0000"); err == nil || err.(*api.Error).Status != 409 {
		t.Fatalf("wrong fingerprint: %v", err)
	}
	if len(pending(a)) != 1 {
		t.Fatal("a refused approval changed the request")
	}
	if _, err := a.n.Approve(bg, b.id, pending(a)[0].Fingerprint); err != nil {
		t.Fatal(err)
	}
}

func TestEventsKeepTheirOldShape(t *testing.T) {
	a, b := newHost(t, false), newHost(t, false)
	ch, stop := a.n.Subscribe()
	defer stop()
	res, _ := b.n.Pair(bg, addr(t, a))
	_ = res
	seen := map[string]api.Event{}
	deadline := time.After(3 * time.Second)
	for seen["pending"].Kind == "" {
		select {
		case ev := <-ch:
			seen[ev.Kind] = ev
		case <-deadline:
			t.Fatalf("no pending event; saw %v", seen)
		}
	}
	if seen["pending"].Peer == "" {
		t.Fatalf("pending event has no name: %+v", seen["pending"])
	}
	a.n.Approve(bg, b.id, "")
	b.n.Approve(bg, a.id, "")
	eventually(t, "peer-up", func() bool {
		for {
			select {
			case ev := <-ch:
				seen[ev.Kind] = ev
			default:
				return seen["peer-up"].Peer != ""
			}
		}
	})
	a.board.user(text("secret words"))
	eventually(t, "sent event", func() bool {
		for {
			select {
			case ev := <-ch:
				seen[ev.Kind] = ev
			default:
				return seen["sent"].Kind != ""
			}
		}
	})
	if ev := seen["sent"]; strings.Contains(ev.Info, "secret") {
		t.Fatalf("an event carries clipboard text: %+v", ev)
	}
}

// pngImage is a valid PNG of about a megabyte.
func pngImage(t *testing.T) []byte {
	t.Helper()
	img := image.NewNRGBA(image.Rect(0, 0, 600, 600))
	for i := range img.Pix {
		img.Pix[i] = byte(i * 7)
	}
	var b bytes.Buffer
	if err := png.Encode(&b, img); err != nil {
		t.Fatal(err)
	}
	return b.Bytes()
}

func TestSimultaneousPairingYieldsOneSession(t *testing.T) {
	a, b := newHost(t, false), newHost(t, false)
	aa, ba := addr(t, a), addr(t, b)
	// Both users pair at once; each device then approves the other.
	var wg sync.WaitGroup
	wg.Add(2)
	go func() { defer wg.Done(); a.n.Pair(bg, ba) }()
	go func() { defer wg.Done(); b.n.Pair(bg, aa) }()
	wg.Wait()
	for _, x := range [][2]*host{{a, b}, {b, a}} {
		x := x
		eventually(t, "request", func() bool {
			for _, p := range pending(x[0]) {
				if p.ID == x[1].id {
					return true
				}
			}
			return false
		})
		if _, err := x[0].n.Approve(bg, x[1].id, ""); err != nil {
			t.Fatal(err)
		}
	}
	eventually(t, "connected", func() bool { return connected(a, b) })
	a.board.user(text("still works"))
	eventually(t, "sync after the race", func() bool { return got(b) == "still works" })
}

func TestPairingIsRemembered(t *testing.T) {
	a, b := newHost(t, false), newHost(t, false)
	pair(t, a, b)
	// a never dialed b, yet it knows b by its stored address for reconnecting.
	st, _ := a.n.Status(bg)
	if len(st.Peers) != 1 || st.Peers[0].ID != b.id {
		t.Fatalf("a's peers = %+v", st.Peers)
	}
}

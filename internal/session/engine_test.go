package session

import (
	"context"
	"net"
	"path/filepath"
	"testing"
	"time"

	"github.com/uniclipboard/unic/internal/rendezvous"
	"github.com/uniclipboard/unic/internal/store"
)

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

type node struct{ e *Engine }

func newNode(t *testing.T, id string) node {
	t.Helper()
	return newNodeWith(t, id, nil)
}

func newNodeWith(t *testing.T, id string, rv *rendezvous.Client) node {
	t.Helper()
	st, err := store.Open(filepath.Join(t.TempDir(), "db"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { st.Close() })
	e := New(Config{ID: id, Name: id, Store: st, Transport: newTCP(t), Logf: t.Logf, Rendezvous: rv})
	ctx, cancel := context.WithCancel(context.Background())
	t.Cleanup(cancel)
	go e.Run(ctx)
	return node{e}
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

func TestPairingIsMutualAndRemembered(t *testing.T) {
	a, b := newNode(t, "aaa"), newNode(t, "bbb")
	pair(t, a, b)
	// a never dialed b, yet it knows b for reconnecting after a restart.
	ps, err := a.e.cfg.Store.Peers()
	if err != nil || len(ps) != 1 || ps[0].ID != "bbb" || ps[0].Addr == "" {
		t.Fatalf("a's stored peers = %+v, %v", ps, err)
	}
}

func TestSimultaneousDialYieldsOneSession(t *testing.T) {
	a, b := newNode(t, "aaa"), newNode(t, "bbb")
	time.Sleep(20 * time.Millisecond)
	_, _, aa := a.e.Self()
	_, _, ba := b.e.Self()
	// Both users pair at once: each dial is the user's own decision, so no
	// approval is needed on either side.
	go func() { _, _, _ = a.e.AddPeer(context.Background(), ba) }()
	go func() { _, _, _ = b.e.AddPeer(context.Background(), aa) }()
	eventually(t, "connected", func() bool { return connected(a, b) })
}

func TestUnknownDeviceWaitsForApproval(t *testing.T) {
	a, b := newNode(t, "aaa"), newNode(t, "bbb")
	time.Sleep(20 * time.Millisecond)
	_, _, addr := a.e.Self()
	if _, awaiting, err := b.e.AddPeer(context.Background(), addr); err != nil || !awaiting {
		t.Fatalf("AddPeer = awaiting %v, %v", awaiting, err)
	}
	if ps := pending(a); len(ps) != 1 || ps[0].ID != "bbb" || ps[0].Via != "" {
		t.Fatalf("a's pending = %+v", ps)
	}
	// Not approved: no connection, no sync, in either direction.
	time.Sleep(300 * time.Millisecond)
	if connected(a, b) {
		t.Fatal("unapproved device was connected")
	}
}

func TestIntroductionNeedsApprovalOnEveryDevice(t *testing.T) {
	a, b, c := newNode(t, "aaa"), newNode(t, "bbb"), newNode(t, "ccc")
	pair(t, a, b) // b pairs with a
	pair(t, b, c) // c pairs with b; a and c have never met

	// b introduces the two to each other. Neither connects by itself.
	eventually(t, "a hears about c", func() bool { return len(pending(a)) == 1 })
	eventually(t, "c hears about a", func() bool { return len(pending(c)) == 1 })
	if ps := pending(a); ps[0].ID != "ccc" || ps[0].Via != "bbb" {
		t.Fatalf("a's pending = %+v", ps)
	}
	time.Sleep(300 * time.Millisecond)
	if connected(a, c) {
		t.Fatal("introduced devices connected without approval")
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
	if len(pending(a)) != 0 || len(pending(c)) != 0 {
		t.Fatal("pending not cleared after approval")
	}
}

func TestRejectedDeviceStaysQuietUntilItAsksAgain(t *testing.T) {
	a, b, c := newNode(t, "aaa"), newNode(t, "bbb"), newNode(t, "ccc")
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

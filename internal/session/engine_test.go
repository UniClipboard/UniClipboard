package session

import (
	"context"
	"errors"
	"net"
	"sync"
	"testing"
	"time"

	"github.com/uniclipboard/unic/internal/identity"
	"github.com/uniclipboard/unic/internal/wire"
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

// Binding names the listening address: both ends of a connection know it.
func (t *tcpTransport) Binding(addr string) ([]byte, error) { return []byte("test/1\x00" + addr), nil }

// fakeHost is a device table with the host's three decisions in it.
type fakeHost struct {
	mu       sync.Mutex
	known    map[string]*fakeDev // by device id
	tok      []byte              // the live ticket's token
	ticketOK bool                // Pair succeeded for a verified proof
}

type fakeDev struct {
	pk     []byte
	addr   string
	allow  bool
	paused bool
	ticket bool
}

func (h *fakeHost) Verify(v Verified) (Standing, error) {
	h.mu.Lock()
	defer h.mu.Unlock()
	id := identity.DeviceID(v.PublicKey)
	d := h.known[id]
	if d == nil {
		d = &fakeDev{pk: v.PublicKey}
		h.known[id] = d
	}
	if v.Addr != "" {
		d.addr = v.Addr
	}
	d.ticket = d.ticket || v.Ticket
	return Standing{DeviceID: id, Name: v.Name, Allowed: d.allow, SyncEnabled: !d.paused}, nil
}

func (h *fakeHost) Standing(id string) (Standing, bool) {
	h.mu.Lock()
	defer h.mu.Unlock()
	d := h.known[id]
	if d == nil {
		return Standing{}, false
	}
	return Standing{DeviceID: id, Allowed: d.allow, SyncEnabled: !d.paused}, true
}

func (h *fakeHost) Targets() []Target {
	h.mu.Lock()
	defer h.mu.Unlock()
	var out []Target
	for id, d := range h.known {
		if d.allow && d.addr != "" {
			out = append(out, Target{DeviceID: id, Name: id[:4], Addr: d.addr})
		}
	}
	return out
}

func (h *fakeHost) Pair(t [32]byte, proof []byte) bool {
	h.mu.Lock()
	defer h.mu.Unlock()
	if h.tok == nil || identity.VerifyPairProof(h.tok, t, proof) != nil {
		return false
	}
	h.tok, h.ticketOK = nil, true // one ticket, one use
	return true
}

func (h *fakeHost) Active(string, error) {}

func (h *fakeHost) allow(id string, v bool) {
	h.mu.Lock()
	h.known[id].allow = v
	h.mu.Unlock()
}

func (h *fakeHost) get(id string) (fakeDev, bool) {
	h.mu.Lock()
	defer h.mu.Unlock()
	d := h.known[id]
	if d == nil {
		return fakeDev{}, false
	}
	return *d, true
}

type node struct {
	e    *Engine
	h    *fakeHost
	id   string
	addr string
}

func newNode(t *testing.T) node {
	t.Helper()
	ident, err := identity.FromSeed(randSeed(t))
	if err != nil {
		t.Fatal(err)
	}
	h := &fakeHost{known: map[string]*fakeDev{}}
	tr := newTCP(t)
	e := New(Config{Identity: ident, Name: "n", Host: h, Transport: tr, Logf: t.Logf})
	ctx, cancel := context.WithCancel(context.Background())
	t.Cleanup(cancel)
	ready := make(chan struct{})
	go e.RunReady(ctx, func() { close(ready) })
	<-ready
	return node{e: e, h: h, id: ident.DeviceID(), addr: tr.Addr()}
}

var seedN byte

func randSeed(t *testing.T) []byte {
	seedN++
	s := make([]byte, 32)
	for i := range s {
		s[i] = seedN*7 + byte(i)
	}
	return s
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

func connected(a, b node) bool { return a.e.Connected(b.id) && b.e.Connected(a.id) }

// pair makes b reach a, then both users authorize the other, as two users would.
func pair(t *testing.T, a, b node) {
	t.Helper()
	st, awaiting, err := b.e.Connect(context.Background(), a.addr, a.id, nil)
	if err != nil || awaiting || st.DeviceID != a.id || st.Allowed {
		t.Fatalf("Connect = %+v, awaiting %v, %v", st, awaiting, err)
	}
	eventually(t, "a sees the request", func() bool { _, ok := a.h.get(b.id); return ok })
	a.h.allow(b.id, true)
	b.h.allow(a.id, true)
	a.e.Reconcile()
	b.e.Reconcile()
	eventually(t, "both sides connected", func() bool { return connected(a, b) })
}

func TestPairingNeedsBothSidesAndIsRemembered(t *testing.T) {
	a, b := newNode(t), newNode(t)
	pair(t, a, b)
	// a never dialed b, yet it learned b's address for reconnecting.
	if d, _ := a.h.get(b.id); d.addr == "" {
		t.Fatal("a did not keep b's address")
	}
}

func TestUnknownDeviceWaitsForApproval(t *testing.T) {
	a, b := newNode(t), newNode(t)
	if _, _, err := b.e.Connect(context.Background(), a.addr, a.id, nil); err != nil {
		t.Fatal(err)
	}
	if d, ok := a.h.get(b.id); !ok || d.allow || d.ticket {
		t.Fatalf("a's view of b = %+v, %v", d, ok)
	}
	time.Sleep(300 * time.Millisecond)
	if connected(a, b) || a.e.Connected(b.id) {
		t.Fatal("unapproved device was connected")
	}
}

func TestOneSideApprovingIsNotEnough(t *testing.T) {
	a, b := newNode(t), newNode(t)
	if _, _, err := b.e.Connect(context.Background(), a.addr, a.id, nil); err != nil {
		t.Fatal(err)
	}
	a.h.allow(b.id, true)
	a.e.Reconcile()
	time.Sleep(300 * time.Millisecond)
	if connected(a, b) {
		t.Fatal("connected before b approved a")
	}
	b.h.allow(a.id, true)
	b.e.Reconcile()
	eventually(t, "connected", func() bool { return connected(a, b) })
}

func TestWrongDeviceBehindAddressIsRefused(t *testing.T) {
	a, b, c := newNode(t), newNode(t), newNode(t)
	_, _, err := b.e.Connect(context.Background(), a.addr, c.id, nil)
	if !errors.Is(err, ErrIdentity) {
		t.Fatalf("err = %v, want ErrIdentity", err)
	}
	if _, ok := a.h.get(b.id); ok {
		t.Fatal("a mismatched dial must leave no record on the other side")
	}
}

func TestTicketProofMarksTheRequestButTrustsNothing(t *testing.T) {
	a, b := newNode(t), newNode(t)
	tok := []byte("0123456789abcdef")
	a.h.tok = tok
	if _, _, err := b.e.Connect(context.Background(), a.addr, a.id, tok); err != nil {
		t.Fatal(err)
	}
	if d, ok := a.h.get(b.id); !ok || !d.ticket || d.allow {
		t.Fatalf("a's view of b = %+v, %v", d, ok)
	}
	// The ticket is spent: a second joiner with it is an ordinary stranger.
	c := newNode(t)
	if _, _, err := c.e.Connect(context.Background(), a.addr, a.id, tok); err != nil {
		t.Fatal(err)
	}
	if d, _ := a.h.get(c.id); d.ticket {
		t.Fatal("a used-up ticket marked a second device")
	}
}

func TestWrongTicketTokenIsNotAProof(t *testing.T) {
	a, b := newNode(t), newNode(t)
	a.h.tok = []byte("0123456789abcdef")
	if _, _, err := b.e.Connect(context.Background(), a.addr, a.id, []byte("fedcba9876543210")); err != nil {
		t.Fatal(err)
	}
	if d, _ := a.h.get(b.id); d.ticket {
		t.Fatal("a wrong token marked the request")
	}
	if a.h.tok == nil {
		t.Fatal("a wrong token used up the ticket")
	}
}

func TestRevocationDropsTheSession(t *testing.T) {
	a, b := newNode(t), newNode(t)
	pair(t, a, b)
	a.h.allow(b.id, false)
	a.e.Reconcile()
	eventually(t, "session dropped", func() bool { return !a.e.Connected(b.id) })
	time.Sleep(300 * time.Millisecond)
	if a.e.Connected(b.id) {
		t.Fatal("a revoked device reconnected")
	}
}

func TestSimultaneousDialYieldsOneSession(t *testing.T) {
	a, b := newNode(t), newNode(t)
	a.h.known[b.id] = &fakeDev{allow: true, addr: b.addr}
	b.h.known[a.id] = &fakeDev{allow: true, addr: a.addr}
	go a.e.Reconcile()
	go b.e.Reconcile()
	eventually(t, "connected", func() bool { return connected(a, b) })
}

func TestPausedPathGetsNoClips(t *testing.T) {
	a, b := newNode(t), newNode(t)
	pair(t, a, b)
	a.h.mu.Lock()
	a.h.known[b.id].paused = true
	a.h.mu.Unlock()
	if a.e.Submit(func([]string) (Job, bool) { t.Fatal("prepare ran for a paused path"); return Job{}, false }) {
		t.Fatal("a paused path was queued")
	}
}

// Consecutive sends to one device travel over the same session: no new
// handshake, no new connection.
func TestConsecutiveSendsReuseTheSession(t *testing.T) {
	a, b := newNode(t), newNode(t)
	pair(t, a, b)
	var mu sync.Mutex
	var used []*wire.Conn
	done := make(chan string, 4)
	send := func() {
		ok := a.e.Submit(func(peers []string) (Job, bool) {
			return Job{
				Report: func(peer string, st TargetState, err error) {
					if st == Sent || st == Failed {
						done <- string(st)
					}
				},
				Send: func(_ context.Context, w *wire.Conn, _ string) (bool, error) {
					mu.Lock()
					used = append(used, w)
					mu.Unlock()
					return false, nil
				},
			}, true
		})
		if !ok {
			t.Fatal("not queued")
		}
		select {
		case st := <-done:
			if st != string(Sent) {
				t.Fatalf("state %s", st)
			}
		case <-time.After(5 * time.Second):
			t.Fatal("send did not finish")
		}
	}
	send()
	send()
	send()
	mu.Lock()
	defer mu.Unlock()
	if len(used) != 3 || used[0] != used[1] || used[1] != used[2] {
		t.Fatalf("sessions used: %v", used)
	}
}

// A connection that breaks is replaced by a new one without anyone asking.
func TestBrokenConnectionIsReestablished(t *testing.T) {
	a, b := newNode(t), newNode(t)
	pair(t, a, b)
	a.e.mu.Lock()
	s := a.e.sessions[b.id]
	a.e.mu.Unlock()
	if s == nil {
		t.Fatal("no session")
	}
	s.close()
	eventually(t, "session gone", func() bool {
		a.e.mu.Lock()
		defer a.e.mu.Unlock()
		return a.e.sessions[b.id] != s
	})
	a.e.Reconcile()
	b.e.Reconcile()
	eventually(t, "reconnected", func() bool { return connected(a, b) })
	a.e.mu.Lock()
	n := a.e.sessions[b.id]
	a.e.mu.Unlock()
	if n == s {
		t.Fatal("the old session came back")
	}
}

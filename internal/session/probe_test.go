package session

import (
	"context"
	"errors"
	"net"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/uniclipboard/unic/internal/wire"
	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/plugin/rpc"
)

type probeAuth struct {
	mu     sync.Mutex
	events []rpc.PeerEvent
	evCh   chan rpc.PeerEvent
}

func newProbeAuth() *probeAuth { return &probeAuth{evCh: make(chan rpc.PeerEvent, 32)} }

func (a *probeAuth) IdentityHello(context.Context, rpc.IdentityHello) (rpc.IdentityHelloResult, error) {
	return rpc.IdentityHelloResult{Hello: b64.EncodeToString([]byte("h"))}, nil
}
func (a *probeAuth) IdentityProve(context.Context, rpc.IdentityProve) (rpc.IdentityProveResult, error) {
	return rpc.IdentityProveResult{Proof: b64.EncodeToString([]byte("p"))}, nil
}
func (a *probeAuth) CheckPeer(context.Context, rpc.CheckPeer) (rpc.PeerDecision, error) {
	tok := "tok"
	return rpc.PeerDecision{PeerID: "peer-1", Decision: "allow", SessionToken: &tok, TrustRevision: 1}, nil
}
func (a *probeAuth) PeerEvent(_ context.Context, r rpc.PeerEvent) error {
	a.mu.Lock()
	a.events = append(a.events, r)
	a.mu.Unlock()
	select {
	case a.evCh <- r:
	default:
	}
	return nil
}
func (a *probeAuth) Introduce(context.Context, rpc.Introduce) error { return nil }

type probeTransport struct {
	path      string
	probePath string
	dialFn    func(context.Context, string) (net.Conn, error)
}

func (t *probeTransport) Start() error { return nil }
func (t *probeTransport) Close() error { return nil }
func (t *probeTransport) Listen(context.Context) (net.Listener, error) {
	return &noopListener{done: make(chan struct{})}, nil
}
func (t *probeTransport) Addr() string                                     { return "addr-self" }
func (t *probeTransport) Binding(string) ([]byte, error)                   { return []byte("bind"), nil }
func (t *probeTransport) Path(net.Conn, bool) string                       { return t.path }
func (t *probeTransport) ProbePath(context.Context, net.Conn, bool) string { return t.probePath }
func (t *probeTransport) Dial(ctx context.Context, addr string) (net.Conn, error) {
	if t.dialFn != nil {
		return t.dialFn(ctx, addr)
	}
	return nil, errors.New("unreachable")
}

type noopListener struct {
	done chan struct{}
	once sync.Once
}

func (l *noopListener) Accept() (net.Conn, error) { <-l.done; return nil, net.ErrClosed }
func (l *noopListener) Close() error              { l.once.Do(func() { close(l.done) }); return nil }
func (l *noopListener) Addr() net.Addr            { return &net.TCPAddr{} }

func attachTestSession(t *testing.T, b *Backend, peerID string, c net.Conn, outgoing bool) *session {
	t.Helper()
	r := b.running()
	if r == nil {
		t.Fatal("backend not running")
	}
	bc := &bodyDeadlineConn{Conn: c}
	s := &session{
		b:        b,
		r:        r,
		conn:     c,
		w:        wire.NewConn(bc),
		bodyConn: bc,
		connID:   randHex(8),
		token:    "tok",
		peerID:   peerID,
		outgoing: outgoing,
		trustRev: 1,
		wlock:    make(chan struct{}, 1),
		pongSem:  make(chan struct{}, 1),
		done:     make(chan struct{}),
		exited:   make(chan struct{}),
	}
	bc.lastRxNs = &s.lastRxUnixNs
	bc.lastTxNs = &s.lastTxUnixNs
	s.markRx()
	s.markTx()
	if !b.establish(r, s) {
		t.Fatal("failed to establish session")
	}
	return s
}

func newRunningBackend(t *testing.T, tr Transport, probeTimeout time.Duration) (*Backend, *probeAuth) {
	t.Helper()
	auth := newProbeAuth()
	b := New(Config{Transport: tr, ProbeTimeout: probeTimeout})
	b.SetPeerAuthority(auth)
	if err := b.Bind(context.Background(), rpc.Bind{
		BindingID: "b1", SelfID: "self", SelfName: "self", WireVersion: rpc.WireVersion,
		BackendID: "builtin.tailcat", TransportKind: plugin.TransportPeerSessions,
	}); err != nil {
		t.Fatal(err)
	}
	st := rpc.SyncState{
		BackendID: "builtin.tailcat", BindingID: "b1", Revision: 1, Runtime: "active",
		DeviceName: "self", SyncEnabled: true, NetworkGeneration: 1,
		Peers:    []rpc.StatePeer{{ID: "peer-1", Name: "Peer 1", Address: "addr-1", TrustRevision: 1}},
		Rejected: []rpc.StateRejected{},
	}
	if err := b.ApplyState(context.Background(), st); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = b.Close(context.Background()) })
	return b, auth
}

func TestPingPongAndProbePeers(t *testing.T) {
	trA := &probeTransport{path: rpc.LinkPathUnknown, probePath: rpc.LinkPathDirect}
	trB := &probeTransport{path: rpc.LinkPathDERP, probePath: rpc.LinkPathDERP}
	bA, _ := newRunningBackend(t, trA, 2*time.Second)
	bB, _ := newRunningBackend(t, trB, 2*time.Second)

	cA, cB := net.Pipe()
	defer func() { _ = cA.Close(); _ = cB.Close() }()
	attachTestSession(t, bA, "peer-1", cA, true)
	attachTestSession(t, bB, "peer-1", cB, false)

	res, err := bA.ProbePeers(context.Background(), []string{"peer-1"})
	if err != nil {
		t.Fatalf("ProbePeers: %v", err)
	}
	if len(res) != 1 {
		t.Fatalf("got %d results, want 1", len(res))
	}
	got := res[0]
	if got.State != rpc.LinkStateConnected {
		t.Fatalf("State = %q, want connected", got.State)
	}
	if got.Path != rpc.LinkPathDirect {
		t.Fatalf("Path = %q, want direct", got.Path)
	}
	if got.LatencyMs <= 0 {
		t.Fatalf("LatencyMs = %d, want > 0", got.LatencyMs)
	}
	if got.LastCheckedMs <= 0 {
		t.Fatalf("LastCheckedMs = %d, want > 0", got.LastCheckedMs)
	}

	snap := bA.LinkSnapshots()["peer-1"]
	if snap.Path != rpc.LinkPathDirect || snap.LatencyMs != got.LatencyMs {
		t.Fatalf("LinkSnapshots mismatch: %+v", snap)
	}
}

func TestNetworkChangeHealsHalfOpenSession(t *testing.T) {
	tr := &probeTransport{path: rpc.LinkPathDirect, probePath: rpc.LinkPathDirect}
	b, auth := newRunningBackend(t, tr, 150*time.Millisecond)

	// Simulate a half-open socket: local end is attached to a pipe whose remote
	// peer never reads or replies (like a dead NAT/TCP flow after macOS sleep).
	cLocal, cRemote := net.Pipe()
	defer func() { _ = cLocal.Close(); _ = cRemote.Close() }()
	// Drain remote reads silently without sending pong so writes succeed, mimicking
	// a blackholed socket where TCP write buffers accept small frames without EOF.
	go func() {
		buf := make([]byte, 1024)
		for {
			if _, err := cRemote.Read(buf); err != nil {
				return
			}
		}
	}()
	s := attachTestSession(t, b, "peer-1", cLocal, true)

	// Drain initial connected event.
	select {
	case ev := <-auth.evCh:
		if ev.State != "connected" {
			t.Fatalf("first event = %q, want connected", ev.State)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("timed out waiting for connected event")
	}

	// Simulate host wake / NotifyNetworkChanged (NetworkGeneration bump).
	st := rpc.SyncState{
		BackendID: "builtin.tailcat", BindingID: "b1", Revision: 2, Runtime: "active",
		DeviceName: "self", SyncEnabled: true, NetworkGeneration: 2,
		Peers:    []rpc.StatePeer{{ID: "peer-1", Name: "Peer 1", Address: "addr-1", TrustRevision: 1}},
		Rejected: []rpc.StateRejected{},
	}
	if err := b.ApplyState(context.Background(), st); err != nil {
		t.Fatalf("ApplyState: %v", err)
	}

	// Half-open session must be detected and closed promptly (~150ms), not hang 30s.
	select {
	case <-s.exited:
	case <-time.After(2 * time.Second):
		t.Fatal("half-open session was not torn down after NetworkGeneration bump")
	}
	select {
	case ev := <-auth.evCh:
		if ev.State != "disconnected" {
			t.Fatalf("event = %q, want disconnected", ev.State)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("timed out waiting for disconnected event")
	}
}

func TestProbeDoesNotKillActiveBodyTransfer(t *testing.T) {
	tr := &probeTransport{path: rpc.LinkPathDirect, probePath: rpc.LinkPathDirect}
	b, _ := newRunningBackend(t, tr, 120*time.Millisecond)

	cLocal, cRemote := net.Pipe()
	defer func() { _ = cLocal.Close(); _ = cRemote.Close() }()
	s := attachTestSession(t, b, "peer-1", cLocal, true)

	// Hold s.wlock and mark active body transfer with recent TX progress.
	if !s.acquireWrite(context.Background()) {
		t.Fatal("acquireWrite failed")
	}
	s.bodyActive.Store(true)
	s.markTx()

	done := make(chan struct{})
	var probeErr error
	go func() {
		_, probeErr = s.ping(context.Background())
		close(done)
	}()

	select {
	case <-done:
	case <-time.After(2 * time.Second):
		t.Fatal("ping blocked too long during active body transfer")
	}
	s.bodyActive.Store(false)
	s.releaseWrite()

	if probeErr != nil {
		t.Fatalf("expected active body transfer probe to succeed gracefully, got %v", probeErr)
	}
	if s.isDone() {
		t.Fatal("active body transfer session was falsely closed by probe")
	}
}

func TestDisconnectedPeerSettlesFromConnectingToDisconnected(t *testing.T) {
	var attempts atomic.Int32
	tr := &probeTransport{
		dialFn: func(context.Context, string) (net.Conn, error) {
			attempts.Add(1)
			return nil, errors.New("offline")
		},
	}
	b, _ := newRunningBackend(t, tr, 200*time.Millisecond)

	deadline := time.Now().Add(4 * time.Second)
	for time.Now().Before(deadline) {
		if attempts.Load() >= maxConnectingAttempts {
			st := b.LinkSnapshots()["peer-1"]
			if st.State == rpc.LinkStateDisconnected {
				return
			}
		}
		time.Sleep(50 * time.Millisecond)
	}
	st := b.LinkSnapshots()["peer-1"]
	t.Fatalf("peer did not settle to disconnected after %d attempts: %+v", attempts.Load(), st)
}

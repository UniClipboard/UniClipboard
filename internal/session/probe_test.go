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

func TestOfflinePeerDoesNotFlickerOrSpamOnNetworkChangeAndRetries(t *testing.T) {
	var attempts atomic.Int32
	tr := &probeTransport{
		dialFn: func(context.Context, string) (net.Conn, error) {
			attempts.Add(1)
			return nil, errors.New("offline")
		},
	}
	b, _ := newRunningBackend(t, tr, 200*time.Millisecond)

	var notifyCount atomic.Int32
	b.SetLinkStatusChanged(func(string) {
		notifyCount.Add(1)
	})

	// Wait for the offline peer to settle into disconnected.
	deadline := time.Now().Add(4 * time.Second)
	for time.Now().Before(deadline) {
		if attempts.Load() >= maxConnectingAttempts && b.LinkSnapshots()["peer-1"].State == rpc.LinkStateDisconnected {
			break
		}
		time.Sleep(25 * time.Millisecond)
	}
	if st := b.LinkSnapshots()["peer-1"]; st.State != rpc.LinkStateDisconnected {
		t.Fatalf("peer did not settle to disconnected: %+v", st)
	}

	settledNotify := notifyCount.Load()
	settledAttempts := attempts.Load()

	// Repeated background NetworkGeneration bumps (e.g. Windows netmon churn) must
	// retry dialing in the background without flipping an already-offline peer back
	// to connecting or emitting link-change notifications.
	for rev := uint64(2); rev <= 4; rev++ {
		st := rpc.SyncState{
			BackendID: "builtin.tailcat", BindingID: "b1", Revision: rpc.U64(rev), Runtime: "active",
			DeviceName: "self", SyncEnabled: true, NetworkGeneration: rpc.U64(rev),
			Peers:    []rpc.StatePeer{{ID: "peer-1", Name: "Peer 1", Address: "addr-1", TrustRevision: 1}},
			Rejected: []rpc.StateRejected{},
		}
		if err := b.ApplyState(context.Background(), st); err != nil {
			t.Fatalf("ApplyState rev %d: %v", rev, err)
		}
		if snap := b.LinkSnapshots()["peer-1"]; snap.State != rpc.LinkStateDisconnected {
			t.Fatalf("rev %d flipped settled offline peer to %q, want disconnected", rev, snap.State)
		}
		time.Sleep(80 * time.Millisecond)
	}

	if attempts.Load() <= settledAttempts {
		t.Fatalf("expected background dial retries after NetworkGeneration bumps, attempts stayed at %d", attempts.Load())
	}
	if got := notifyCount.Load(); got != settledNotify {
		t.Fatalf("received %d redundant link-change notifications after settling offline (before=%d, after=%d)", got-settledNotify, settledNotify, got)
	}

	// Explicit manual refresh MUST transition back to connecting and notify.
	res, err := b.ProbePeers(context.Background(), []string{"peer-1"})
	if err != nil {
		t.Fatalf("ProbePeers: %v", err)
	}
	if len(res) != 1 || res[0].State != rpc.LinkStateConnecting {
		t.Fatalf("manual refresh state = %+v, want connecting", res)
	}
	if notifyCount.Load() <= settledNotify {
		t.Fatalf("manual refresh did not emit link-change notification")
	}
}

func TestConnectingStateDoesNotDipBetweenAttempts(t *testing.T) {
	firstStarted := make(chan struct{}, 1)
	releaseFirst := make(chan struct{})
	secondStarted := make(chan struct{}, 1)
	var attemptNum atomic.Int32

	tr := &probeTransport{
		dialFn: func(ctx context.Context, _ string) (net.Conn, error) {
			n := attemptNum.Add(1)
			if n == 1 {
				select {
				case firstStarted <- struct{}{}:
				default:
				}
				select {
				case <-releaseFirst:
				case <-ctx.Done():
				}
				return nil, errors.New("offline")
			}
			if n == 2 {
				select {
				case secondStarted <- struct{}{}:
				default:
				}
				// Hold second attempt briefly so we can inspect state while running.
				select {
				case <-time.After(150 * time.Millisecond):
				case <-ctx.Done():
				}
				return nil, errors.New("offline")
			}
			return nil, errors.New("offline")
		},
	}
	b, _ := newRunningBackend(t, tr, 200*time.Millisecond)

	var states []string
	var mu sync.Mutex
	b.SetLinkStatusChanged(func(id string) {
		st := b.LinkSnapshots()[id].State
		mu.Lock()
		states = append(states, st)
		mu.Unlock()
	})

	select {
	case <-firstStarted:
	case <-time.After(2 * time.Second):
		t.Fatal("first dial did not start")
	}

	// Simulate attempt 1 taking longer than the old 15s grace window by expiring
	// activeUntil while attempt 1 is still in flight, then letting attempt 1 fail
	// before attempt 2 starts.
	b.mu.Lock()
	ds := b.dialing["peer-1"]
	ds.activeUntil = time.Now().Add(-time.Second)
	b.dialing["peer-1"] = ds
	b.mu.Unlock()

	close(releaseFirst)

	// Wait for attempt 2 to actually run so any dip between attempt 1 and attempt 2 is observed.
	select {
	case <-secondStarted:
	case <-time.After(3 * time.Second):
		t.Fatal("second dial did not start")
	}

	// Wait until peer settles to disconnected after attempt 2 finishes.
	deadline := time.Now().Add(4 * time.Second)
	for time.Now().Before(deadline) {
		if b.LinkSnapshots()["peer-1"].State == rpc.LinkStateDisconnected {
			break
		}
		time.Sleep(25 * time.Millisecond)
	}

	mu.Lock()
	got := append([]string(nil), states...)
	mu.Unlock()

	// Must never bounce disconnected -> connecting -> disconnected.
	seenDisconnected := false
	for _, st := range got {
		if st == rpc.LinkStateDisconnected {
			seenDisconnected = true
		} else if st == rpc.LinkStateConnecting && seenDisconnected {
			t.Fatalf("connecting state flickered back after disconnected: %v", got)
		}
	}
}

func TestStaleCancelledDialDoesNotClobberNewDialer(t *testing.T) {
	firstInDial := make(chan struct{}, 1)
	releaseFirst := make(chan struct{})
	secondInDial := make(chan struct{}, 1)
	releaseSecond := make(chan struct{})
	var n atomic.Int32

	tr := &probeTransport{
		dialFn: func(_ context.Context, _ string) (net.Conn, error) {
			switch n.Add(1) {
			case 1:
				firstInDial <- struct{}{}
				<-releaseFirst
				return nil, context.Canceled
			case 2:
				secondInDial <- struct{}{}
				<-releaseSecond
				return nil, errors.New("offline")
			default:
				return nil, errors.New("offline")
			}
		},
	}
	b, _ := newRunningBackend(t, tr, 200*time.Millisecond)
	defer close(releaseSecond)

	select {
	case <-firstInDial:
	case <-time.After(2 * time.Second):
		t.Fatal("first dial did not start")
	}

	// Manual refresh replaces dialer 1 with dialer 2 while dialer 1 is still blocked in Dial.
	_, err := b.ProbePeers(context.Background(), []string{"peer-1"})
	if err != nil {
		t.Fatalf("ProbePeers: %v", err)
	}

	select {
	case <-secondInDial:
	case <-time.After(2 * time.Second):
		t.Fatal("second dial did not start")
	}

	// Now let stale dialer 1 finish unwinding. It must NOT clear dialer 2's inFlight.
	close(releaseFirst)
	time.Sleep(50 * time.Millisecond)

	b.mu.Lock()
	ds := b.dialing["peer-1"]
	b.mu.Unlock()
	if !ds.inFlight {
		t.Fatalf("stale dialer clobbered active dialer's inFlight flag: %+v", ds)
	}
}

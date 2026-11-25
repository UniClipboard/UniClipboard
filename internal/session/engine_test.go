package session

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"io"
	"net"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/uniclipboard/unic/internal/syncdiag"
	"github.com/uniclipboard/unic/internal/wire"
	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/plugin/rpc"
)

// blockedNet is a Transport whose Start waits until Close.
type blockedNet struct{ stop chan struct{} }

func (n *blockedNet) Start() error                                   { <-n.stop; return errors.New("closed") }
func (n *blockedNet) Close() error                                   { close(n.stop); return nil }
func (n *blockedNet) Listen(context.Context) (net.Listener, error)   { return nil, errors.New("down") }
func (n *blockedNet) Dial(context.Context, string) (net.Conn, error) { return nil, errors.New("down") }
func (n *blockedNet) Addr() string                                   { return "" }
func (n *blockedNet) Binding(string) ([]byte, error)                 { return nil, errors.New("down") }

// Close must cut a Start that is still waiting for the network short instead
// of waiting for it.
func TestCloseDoesNotWaitForStart(t *testing.T) {
	tr := &blockedNet{stop: make(chan struct{})}
	b := New(Config{Transport: tr})
	ctx, cancel := context.WithTimeout(context.Background(), 50*time.Millisecond)
	defer cancel()
	_ = b.up(ctx) // Start is still blocked when the context ends
	done := make(chan error, 1)
	go func() { done <- b.Close(context.Background()) }()
	select {
	case err := <-done:
		if err != nil {
			t.Fatal(err)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("Close waited for a blocked Start")
	}
}

type introAuthority struct {
	plugin.PeerAuthority
	got chan rpc.Introduce
}

func (a introAuthority) Introduce(_ context.Context, r rpc.Introduce) error { a.got <- r; return nil }

func TestIntroductionsBatchAndExcludeSelfAndRecipient(t *testing.T) {
	b := New(Config{})
	b.self = "self"
	b.st.Introduce = true
	b.st.Peers = append(b.st.Peers, rpc.StatePeer{ID: "recipient", TrustRevision: 1}, rpc.StatePeer{ID: "self", Address: "secret-self"})
	for i := 0; i < 130; i++ {
		b.st.Peers = append(b.st.Peers, rpc.StatePeer{ID: fmt.Sprintf("device-%d", i), Name: "phone", Address: "secret-address"})
	}
	left, right := net.Pipe()
	defer func() { _ = left.Close() }()
	defer func() { _ = right.Close() }()
	s := &session{b: b, peerID: "recipient", trustRev: 1, conn: left, w: wire.NewConn(left), done: make(chan struct{})}
	b.sessions[s.peerID] = s
	done := make(chan error, 1)
	go func() { done <- s.sendIntroductions() }()
	_ = right.SetReadDeadline(time.Now().Add(5 * time.Second))
	w := wire.NewConn(right)
	for _, want := range []int{64, 64, 2} {
		var msg introduction
		if err := readTyped(w, "introduce", &msg); err != nil {
			t.Fatal(err)
		}
		if len(msg.Candidates) != want {
			t.Fatalf("batch size: %d", len(msg.Candidates))
		}
		for _, c := range msg.Candidates {
			if c.ID == "self" || c.ID == "recipient" {
				t.Fatalf("introduced excluded device: %s", c.ID)
			}
		}
	}
	if err := <-done; err != nil {
		t.Fatal(err)
	}
	// Both disabling introductions and losing the recipient's authority must
	// return without writing anything, even when the peer is not reading.
	b.st.Introduce = false
	if err := s.sendIntroductions(); err != nil {
		t.Fatal(err)
	}
	b.st.Introduce = true
	b.st.Peers[0].TrustRevision++
	if err := s.sendIntroductions(); err != nil {
		t.Fatal(err)
	}
}

func TestReceiveIntroductionsUsesLocalSessionAndHonorsSwitch(t *testing.T) {
	for _, enabled := range []bool{false, true} {
		t.Run(fmt.Sprint(enabled), func(t *testing.T) {
			b := New(Config{})
			b.st.Introduce = enabled
			got := make(chan rpc.Introduce, 1)
			b.SetPeerAuthority(introAuthority{got: got})
			left, right := net.Pipe()
			defer func() { _ = left.Close() }()
			defer func() { _ = right.Close() }()
			s := &session{b: b, r: &runState{ctx: context.Background()}, conn: left, w: wire.NewConn(left), connID: "local-connection", token: strings.Repeat("a", 64)}
			done := make(chan error, 1)
			go func() { done <- b.recvLoop(s) }()
			if err := wire.NewConn(right).WriteJSON(introduction{T: "introduce", Candidates: []rpc.Candidate{{ID: "candidate", Name: "phone", Address: "secret"}}}); err != nil {
				t.Fatal(err)
			}
			_ = right.Close()
			err := <-done
			_ = err // This scenario intentionally closes the peer connection.
			select {
			case req := <-got:
				if !enabled || req.ConnectionID != s.connID || req.SessionToken != s.token || len(req.Candidates) != 1 {
					t.Fatalf("wrong forwarding: %+v", req)
				}
			default:
				if enabled {
					t.Fatal("introduction not forwarded")
				}
			}
		})
	}
}

type fakeTimeoutErr struct{}

func (fakeTimeoutErr) Error() string   { return "i/o timeout" }
func (fakeTimeoutErr) Timeout() bool   { return true }
func (fakeTimeoutErr) Temporary() bool { return true }

type writeFailConn struct {
	net.Conn
	err error
}

func (c *writeFailConn) Write([]byte) (int, error) { return 0, c.err }

type stubSendSource struct{ data []byte }

func (s stubSendSource) Authorize(context.Context, rpc.Authorize) (rpc.SendGrant, error) {
	return rpc.SendGrant{GrantID: "g1", StateRevision: 1}, nil
}
func (s stubSendSource) OpenPart(context.Context, string, int) (io.ReadCloser, error) {
	return io.NopCloser(bytes.NewReader(s.data)), nil
}

type recordingObserver struct {
	mu      sync.Mutex
	records []syncdiag.Record
}

func (o *recordingObserver) SyncEpoch() uint64 { return 1 }
func (o *recordingObserver) WakeSync()         {}
func (o *recordingObserver) RecordSync(r syncdiag.Record) {
	o.mu.Lock()
	o.records = append(o.records, r)
	o.mu.Unlock()
}

func TestSendTimeoutReturnsTimeoutCodeAndSessionReason(t *testing.T) {
	left, right := net.Pipe()
	defer func() { _ = left.Close() }()
	defer func() { _ = right.Close() }()

	obs := &recordingObserver{}
	b := New(Config{Observer: obs})
	tok := strings.Repeat("a", 64)
	fc := &writeFailConn{Conn: left, err: fakeTimeoutErr{}}
	bc := &bodyDeadlineConn{Conn: fc}
	s := &session{
		b: b, r: &runState{ctx: context.Background()},
		conn: fc, w: wire.NewConn(bc), bodyConn: bc,
		connID: "c1", token: tok, peerID: "p1",
		done: make(chan struct{}), exited: make(chan struct{}),
	}
	b.sessions["p1"] = s

	err := b.Send(context.Background(), rpc.Send{
		DispatchID: "d1", OperationID: "op1", PeerID: "p1",
		ConnectionID: "c1", SessionToken: tok, ContentHandle: "h1",
		Clip: rpc.Clip{
			WireID: "w1", Origin: "self", Hash: "abc",
			Parts: []rpc.ClipPart{{Index: 0, Kind: "file", Name: "big.bin", Size: 4}},
		},
	}, stubSendSource{data: []byte("data")})

	var re *rpc.Error
	if !errors.As(err, &re) || re.Code != rpc.CodeTimeout {
		t.Fatalf("Send error = %v, want rpc.CodeTimeout", err)
	}
	s.endMu.Lock()
	gotReason := s.endReason
	s.endMu.Unlock()
	if gotReason != "timeout" {
		t.Fatalf("session endReason = %q, want timeout", gotReason)
	}
}

type stubPeerReceiver struct {
	recvErr error
	called  bool
}

func (r *stubPeerReceiver) OpenReceive(context.Context, rpc.ReceiveOpen) (rpc.ReceiveGrant, error) {
	id := "r1"
	return rpc.ReceiveGrant{Decision: rpc.DecisionAccept, ReceiveID: &id}, nil
}

func (r *stubPeerReceiver) Receive(context.Context, string, plugin.ReceiveParts) (string, error) {
	r.called = true
	return "", r.recvErr
}

func TestReceivePreservesTimeoutErrorForSessionReason(t *testing.T) {
	left, right := net.Pipe()
	defer func() { _ = left.Close() }()
	defer func() { _ = right.Close() }()

	b := New(Config{})
	recv := &stubPeerReceiver{recvErr: fakeTimeoutErr{}}
	b.SetPeerReceiver(recv)
	bc := &bodyDeadlineConn{Conn: left}
	s := &session{
		b: b, r: &runState{ctx: context.Background()},
		conn: left, w: wire.NewConn(bc), bodyConn: bc,
		connID: "c1", token: strings.Repeat("b", 64), peerID: "p1",
		done: make(chan struct{}), exited: make(chan struct{}),
	}
	clip := &wire.Clip{
		T: "clip", ID: "w1", Origin: "p1", Hash: strings.Repeat("a", 64),
		Parts: []wire.PartMeta{{Kind: "file", Name: "big.bin", Size: 4}},
	}
	err := b.receive(s, clip)
	if !recv.called {
		t.Fatalf("PeerReceiver.Receive was not called (err=%v)", err)
	}
	if got := reason(err); got != "timeout" {
		t.Fatalf("reason(receive err) = %q (err=%v), want timeout", got, err)
	}
}

type gatedTransport struct {
	mu       sync.Mutex
	startCh  chan error
	starts   int
	listens  int
	closed   bool
	addr     string
	listenCh chan struct{}
}

func (g *gatedTransport) Start() error {
	g.mu.Lock()
	g.starts++
	ch := g.startCh
	g.mu.Unlock()
	err := <-ch
	if err == nil {
		g.mu.Lock()
		g.addr = "127.0.0.1:9999"
		g.mu.Unlock()
	}
	return err
}

func (g *gatedTransport) Close() error {
	g.mu.Lock()
	g.closed = true
	g.mu.Unlock()
	return nil
}

func (g *gatedTransport) Listen(ctx context.Context) (net.Listener, error) {
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		return nil, err
	}
	g.mu.Lock()
	g.listens++
	g.addr = ln.Addr().String()
	ch := g.listenCh
	g.mu.Unlock()
	if ch != nil {
		select {
		case ch <- struct{}{}:
		default:
		}
	}
	return ln, nil
}

func (g *gatedTransport) Dial(ctx context.Context, addr string) (net.Conn, error) {
	return nil, errors.New("unused")
}

func (g *gatedTransport) Addr() string {
	g.mu.Lock()
	defer g.mu.Unlock()
	return g.addr
}

func (g *gatedTransport) Binding(string) ([]byte, error) { return []byte("bind"), nil }

func TestApplyStateStartsTransportAsynchronously(t *testing.T) {
	tr := &gatedTransport{
		startCh:  make(chan error, 1),
		listenCh: make(chan struct{}, 2),
	}
	b := New(Config{Transport: tr})
	if err := b.Bind(context.Background(), rpc.Bind{BindingID: "b1", SelfID: "self", SelfName: "self", WireVersion: 1, BackendID: "builtin.tailcat", TransportKind: "peer_sessions"}); err != nil {
		t.Fatal(err)
	}
	defer func() { _ = b.Close(context.Background()) }()

	// ApplyState(active) must return immediately even while Transport.Start is blocked.
	start := time.Now()
	if err := b.ApplyState(context.Background(), rpc.SyncState{BackendID: "builtin.tailcat", BindingID: "b1", Runtime: "active", Revision: 1}); err != nil {
		t.Fatalf("ApplyState: %v", err)
	}
	if elapsed := time.Since(start); elapsed > 200*time.Millisecond {
		t.Fatalf("ApplyState blocked for %v while Transport.Start was pending", elapsed)
	}

	// PairingAddress respects caller cancellation while Transport.Start is pending.
	cancelCtx, cancel := context.WithTimeout(context.Background(), 40*time.Millisecond)
	defer cancel()
	if _, err := b.PairingAddress(cancelCtx, "export"); err == nil {
		t.Fatal("expected cancelled PairingAddress while Transport.Start is blocked")
	}

	// PairingAddress waits for Transport.Start and succeeds once it completes.
	addrCh := make(chan string, 1)
	errCh := make(chan error, 1)
	go func() {
		addr, err := b.PairingAddress(context.Background(), "export")
		addrCh <- addr
		errCh <- err
	}()
	tr.startCh <- nil
	if err := <-errCh; err != nil {
		t.Fatalf("PairingAddress after start: %v", err)
	}
	if got := <-addrCh; got == "" {
		t.Fatal("expected non-empty pairing address")
	}
	select {
	case <-tr.listenCh:
	case <-time.After(2 * time.Second):
		t.Fatal("listener did not start after Transport.Start succeeded")
	}
}

func TestApplyStateRetriesAfterAsyncTransportStartFailure(t *testing.T) {
	tr := &gatedTransport{
		startCh:  make(chan error, 2),
		listenCh: make(chan struct{}, 2),
	}
	b := New(Config{Transport: tr})
	if err := b.Bind(context.Background(), rpc.Bind{BindingID: "b1", SelfID: "self", SelfName: "self", WireVersion: 1, BackendID: "builtin.tailcat", TransportKind: "peer_sessions"}); err != nil {
		t.Fatal(err)
	}
	defer func() { _ = b.Close(context.Background()) }()

	tr.startCh <- errors.New("temporary network failure")
	if err := b.ApplyState(context.Background(), rpc.SyncState{BackendID: "builtin.tailcat", BindingID: "b1", Runtime: "active", Revision: 1}); err != nil {
		t.Fatalf("ApplyState #1: %v", err)
	}
	// Wait for the first Start attempt to complete and fail.
	if _, err := b.PairingAddress(context.Background(), "export"); err == nil {
		t.Fatal("expected PairingAddress to fail after failed Transport.Start")
	}

	// A later active ApplyState must retry Transport.Start and bring up the listener.
	tr.startCh <- nil
	if err := b.ApplyState(context.Background(), rpc.SyncState{BackendID: "builtin.tailcat", BindingID: "b1", Runtime: "active", Revision: 2, NetworkGeneration: 1}); err != nil {
		t.Fatalf("ApplyState #2: %v", err)
	}
	select {
	case <-tr.listenCh:
	case <-time.After(2 * time.Second):
		t.Fatal("listener did not start on retry after failed Transport.Start")
	}
	if addr, err := b.PairingAddress(context.Background(), "export"); err != nil || addr == "" {
		t.Fatalf("PairingAddress after retry = (%q, %v)", addr, err)
	}
}

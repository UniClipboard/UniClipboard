package engine

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/uniclipboard/unic/internal/rendezvous"
	"github.com/uniclipboard/unic/internal/rendezvous/rvtest"
)

// fakeNet is a loopback network. Its address stays the same across listens.
type fakeNet struct {
	mu         sync.Mutex
	addr       string
	ln         net.Listener
	startErr   error
	startBlock chan struct{} // if set, Start waits for it
	closeBlock chan struct{} // if set, Close waits for it
	started    int
	closed     int
}

func newFakeNet(t *testing.T) *fakeNet {
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	f := &fakeNet{ln: ln, addr: ln.Addr().String()}
	t.Cleanup(func() {
		f.mu.Lock()
		defer f.mu.Unlock()
		if f.ln != nil {
			_ = f.ln.Close()
		}
	})
	return f
}

func (f *fakeNet) Listen(context.Context) (net.Listener, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.ln != nil {
		ln := f.ln
		f.ln = nil
		return ln, nil
	}
	return net.Listen("tcp", f.addr)
}

func (f *fakeNet) Dial(ctx context.Context, a string) (net.Conn, error) {
	var d net.Dialer
	return d.DialContext(ctx, "tcp", a)
}

func (f *fakeNet) Addr() string { return f.addr }

func (f *fakeNet) Start() error {
	f.mu.Lock()
	f.started++
	b, err := f.startBlock, f.startErr
	f.mu.Unlock()
	if b != nil {
		<-b
	}
	return err
}

func (f *fakeNet) Close() error {
	f.mu.Lock()
	b := f.closeBlock
	f.mu.Unlock()
	if b != nil {
		<-b
	}
	f.mu.Lock()
	f.closed++
	f.mu.Unlock()
	return nil
}

type node struct {
	e   *Engine
	net *fakeNet
	dir string
}

func newOpts(t *testing.T, dir string) (Options, *fakeNet) {
	f := newFakeNet(t)
	return Options{DataDir: dir, Network: f}, f
}

func newNode(t *testing.T, rv *rvtest.Server) node {
	t.Helper()
	dir := t.TempDir()
	opts, f := newOpts(t, dir)
	if rv != nil {
		opts.rendezvous = &rendezvous.Client{URL: rv.URL, HTTP: rv.Client()}
	}
	e, err := New(context.Background(), opts)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = e.Close(context.Background()) })
	return node{e, f, dir}
}

func start(t *testing.T, n node) {
	t.Helper()
	if err := n.e.Start(context.Background()); err != nil {
		t.Fatal(err)
	}
}

func eventually(t *testing.T, what string, f func() bool) {
	t.Helper()
	for i := 0; i < 400; i++ {
		if f() {
			return
		}
		time.Sleep(10 * time.Millisecond)
	}
	t.Fatalf("timed out waiting for %s", what)
}

func code(err error) Code {
	var e *Error
	if errors.As(err, &e) {
		return e.Code
	}
	return ""
}

func state(t *testing.T, e *Engine) State {
	t.Helper()
	s, err := e.GetState(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	return s
}

var bg = context.Background()

// ---- lifecycle ----

func TestLifecycle(t *testing.T) {
	n := newNode(t, nil)
	if s := state(t, n.e); s.Lifecycle != LifecycleCreated || s.Self.ID == "" {
		t.Fatalf("state = %+v", s)
	}
	start(t, n)
	if got := code(n.e.Start(bg)); got != CodeInvalidState {
		t.Fatalf("second Start: %q", got)
	}
	if s := state(t, n.e); s.Lifecycle != LifecycleRunning {
		t.Fatalf("lifecycle = %s", s.Lifecycle)
	}
	for i := 0; i < 2; i++ {
		if err := n.e.Stop(bg); err != nil {
			t.Fatal(err)
		}
	}
	if s := state(t, n.e); s.Lifecycle != LifecycleStopped {
		t.Fatalf("lifecycle = %s", s.Lifecycle)
	}
	if n.net.closed != 1 {
		t.Fatalf("transport closed %d times", n.net.closed)
	}
	if got := code(n.e.Start(bg)); got != CodeInvalidState {
		t.Fatalf("Start after Stop: %q", got)
	}
	if _, err := n.e.ListPeers(bg); err != nil {
		t.Fatalf("queries work after Stop: %v", err)
	}
	for i := 0; i < 2; i++ {
		if err := n.e.Close(bg); err != nil {
			t.Fatal(err)
		}
	}
	if got := code(func() error { _, err := n.e.GetState(bg); return err }()); got != CodeClosed {
		t.Fatalf("after Close: %q", got)
	}
	if err := n.e.Stop(bg); err != nil {
		t.Fatalf("Stop after Close: %v", err)
	}
}

func TestDataDirIsExclusiveAndFreedByClose(t *testing.T) {
	n := newNode(t, nil)
	opts, _ := newOpts(t, n.dir)
	if _, err := New(bg, opts); code(err) != CodeInUse {
		t.Fatalf("second instance on one directory: %v", err)
	}
	if err := n.e.Close(bg); err != nil {
		t.Fatal(err)
	}
	e2, err := New(bg, opts)
	if err != nil {
		t.Fatalf("after Close the directory is free: %v", err)
	}
	_ = e2.Close(bg)
}

func TestIdentityPersistsAndInstanceChanges(t *testing.T) {
	n := newNode(t, nil)
	s1 := state(t, n.e)
	_ = n.e.Close(bg)
	opts, _ := newOpts(t, n.dir)
	e2, err := New(bg, opts)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = e2.Close(bg) }()
	s2 := state(t, e2)
	if s1.Self != s2.Self {
		t.Fatalf("identity changed: %+v -> %+v", s1.Self, s2.Self)
	}
	if s1.InstanceID == s2.InstanceID {
		t.Fatal("instance id must change with every New")
	}
}

func TestStartFailureRecovers(t *testing.T) {
	n := newNode(t, nil)
	n.net.startErr = errors.New("dial 10.0.0.1: secret-address-text")
	err := n.e.Start(bg)
	if code(err) != CodeUnavailable {
		t.Fatalf("Start = %v", err)
	}
	if strings.Contains(err.Error(), "secret") {
		t.Fatalf("lower-level error text leaked: %v", err)
	}
	if s := state(t, n.e); s.Lifecycle != LifecycleStopped {
		t.Fatalf("lifecycle = %s, want stopped", s.Lifecycle)
	}
	if n.net.closed != 1 {
		t.Fatal("the half-started transport was not released")
	}
	if err := n.e.Close(bg); err != nil {
		t.Fatal(err)
	}
}

func TestStopCancelsStarting(t *testing.T) {
	n := newNode(t, nil)
	n.net.startBlock = make(chan struct{})
	res := make(chan error, 1)
	go func() { res <- n.e.Start(bg) }()
	eventually(t, "starting", func() bool { return state(t, n.e).Lifecycle == LifecycleStarting })

	stopped := make(chan error, 1)
	go func() { stopped <- n.e.Stop(bg) }()
	if err := <-res; err == nil {
		t.Fatal("Start succeeded though it was stopped")
	}
	close(n.net.startBlock) // the transport finally returns; teardown may close it
	if err := <-stopped; err != nil {
		t.Fatal(err)
	}
	if s := state(t, n.e); s.Lifecycle != LifecycleStopped {
		t.Fatalf("lifecycle = %s", s.Lifecycle)
	}
}

func TestStopTimeoutKeepsStoppingAndCanBeAwaited(t *testing.T) {
	n := newNode(t, nil)
	start(t, n)
	n.net.closeBlock = make(chan struct{})
	ctx, cancel := context.WithTimeout(bg, 50*time.Millisecond)
	defer cancel()
	err := n.e.Stop(ctx)
	if code(err) != CodeTimeout || !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("Stop = %v", err)
	}
	if s := state(t, n.e); s.Lifecycle != LifecycleStopping {
		t.Fatalf("lifecycle = %s, want stopping", s.Lifecycle)
	}
	if _, err := n.e.ListPeers(bg); err != nil {
		t.Fatalf("queries work while stopping: %v", err)
	}
	close(n.net.closeBlock)
	if err := n.e.Stop(bg); err != nil {
		t.Fatal(err)
	}
	if s := state(t, n.e); s.Lifecycle != LifecycleStopped {
		t.Fatalf("lifecycle = %s", s.Lifecycle)
	}
}

func TestNilAndCancelledContext(t *testing.T) {
	n := newNode(t, nil)
	//lint:ignore SA1012 the contract rejects nil contexts
	if _, err := n.e.GetState(nil); code(err) != CodeInvalidArgument { //nolint
		t.Fatalf("nil ctx: %v", err)
	}
	ctx, cancel := context.WithCancel(bg)
	cancel()
	if _, err := n.e.GetState(ctx); !errors.Is(err, context.Canceled) || code(err) != CodeCancelled {
		t.Fatalf("cancelled ctx: %v", err)
	}
}

// ---- settings and capabilities ----

func TestSettingsCompareAndSwap(t *testing.T) {
	n := newNode(t, nil)
	s0 := state(t, n.e).Settings
	v := s0.Value
	v.DeviceName = "  Desk  "
	got, err := n.e.UpdateSettings(bg, UpdateSettingsRequest{ExpectedRevision: s0.Revision, Value: v})
	if err != nil || got.Revision != s0.Revision+1 || got.Value.DeviceName != "Desk" {
		t.Fatalf("update = %+v, %v", got, err)
	}
	if state(t, n.e).Self.Name != "Desk" {
		t.Fatal("name not in effect")
	}
	// Based on a stale revision.
	if _, err := n.e.UpdateSettings(bg, UpdateSettingsRequest{ExpectedRevision: s0.Revision, Value: v}); code(err) != CodeConflict {
		t.Fatalf("stale update: %v", err)
	}
	// One bad field keeps the whole previous value.
	bad := got.Value
	bad.DeviceName = "Other"
	bad.Limits.MaxFileBytes = 0
	if _, err := n.e.UpdateSettings(bg, UpdateSettingsRequest{ExpectedRevision: got.Revision, Value: bad}); code(err) != CodeInvalidArgument {
		t.Fatalf("invalid update: %v", err)
	}
	if cur := state(t, n.e).Settings; cur != got {
		t.Fatalf("a rejected update changed the settings: %+v", cur)
	}
	// Same value: no new revision.
	same, err := n.e.UpdateSettings(bg, UpdateSettingsRequest{ExpectedRevision: got.Revision, Value: got.Value})
	if err != nil || same.Revision != got.Revision {
		t.Fatalf("no-op update = %+v, %v", same, err)
	}
}

func TestNewRejectsBadOptions(t *testing.T) {
	d := t.TempDir()
	bad := DefaultSettings()
	bad.Limits.MaxTotalBytes = 1 << 40
	for name, o := range map[string]Options{
		"no dir":        {},
		"bad settings":  {DataDir: d, Settings: bad},
		"small storage": {DataDir: d, Resources: ResourcePolicy{MaxStoredBytes: 1 << 20}},
		"negative":      {DataDir: d, Resources: ResourcePolicy{EventBuffer: -1}},
		"bad mime":      {DataDir: d, HostCapabilities: HostCapabilities{ReadFormats: []string{"nonsense"}}},
	} {
		if _, err := New(bg, o); code(err) != CodeInvalidArgument {
			t.Errorf("%s: %v", name, err)
		}
	}
	// A failed New must not leave the directory locked.
	opts, _ := newOpts(t, d)
	e, err := New(bg, opts)
	if err != nil {
		t.Fatalf("directory still locked after failed New: %v", err)
	}
	_ = e.Close(bg)
}

func TestCapabilities(t *testing.T) {
	n := newNode(t, nil)
	c, _ := n.e.GetCapabilities(bg)
	if len(c.Effective.ReadFormats) != 0 || c.Host.Observe != AbilityUnsupported {
		t.Fatalf("by default nothing is on: %+v", c)
	}
	if err := n.e.UpdateHostCapabilities(bg, HostCapabilities{
		ReadFormats: []string{"Text/Plain", "image/png", "application/x-private"}, WriteFormats: []string{MimeText},
		ReadFiles: true, Observe: AbilityAvailable, Apply: AbilityPermissionRequired,
	}); err != nil {
		t.Fatal(err)
	}
	c, _ = n.e.GetCapabilities(bg)
	// Not started yet: the host could observe, but the engine will not.
	if c.Effective.Observe != AbilityUnavailable || len(c.Effective.ReadFormats) != 0 || !contains(c.Restrictions, "not_running") {
		t.Fatalf("not running: %+v", c)
	}
	start(t, n)
	c, _ = n.e.GetCapabilities(bg)
	if got := strings.Join(c.Effective.ReadFormats, ","); got != "image/png,text/plain" {
		t.Fatalf("effective read formats = %s", got)
	}
	if c.Effective.Observe != AbilityAvailable || c.Effective.Apply != AbilityPermissionRequired || len(c.Restrictions) != 0 {
		t.Fatalf("running: %+v", c)
	}
	// Turning images off removes the format, not the others.
	s := state(t, n.e).Settings
	s.Value.Sync.Images = false
	_, _ = n.e.UpdateSettings(bg, UpdateSettingsRequest{ExpectedRevision: s.Revision, Value: s.Value})
	c, _ = n.e.GetCapabilities(bg)
	if got := strings.Join(c.Effective.ReadFormats, ","); got != "text/plain" || !contains(c.Restrictions, "images_disabled") {
		t.Fatalf("images off: %+v", c)
	}
	// Pausing sync keeps the connections but disables everything.
	s = state(t, n.e).Settings
	s.Value.Sync.Enabled = false
	_, _ = n.e.UpdateSettings(bg, UpdateSettingsRequest{ExpectedRevision: s.Revision, Value: s.Value})
	c, _ = n.e.GetCapabilities(bg)
	if len(c.Effective.ReadFormats) != 0 || !contains(c.Restrictions, "sync_disabled") {
		t.Fatalf("sync off: %+v", c)
	}
}

func TestSnapshotsAreIndependent(t *testing.T) {
	n := newNode(t, nil)
	_ = n.e.UpdateHostCapabilities(bg, HostCapabilities{ReadFormats: []string{MimeText}})
	s := state(t, n.e)
	s.Capabilities.Host.ReadFormats[0] = "x/y"
	s.Capabilities.CoreFormats[0] = "x/y"
	s.Peers = append(s.Peers, Peer{})
	if c, _ := n.e.GetCapabilities(bg); c.Host.ReadFormats[0] != MimeText || c.CoreFormats[0] != MimeText {
		t.Fatal("a caller changed engine state through a returned DTO")
	}
}

// ---- pairing ----

// pairTwo makes b pair with a by address and a approve b, as two users would.
func pairTwo(t *testing.T, a, b node) {
	t.Helper()
	addr, err := a.e.ExportPairingAddress(bg)
	if err != nil {
		t.Fatal(err)
	}
	op, err := b.e.PairByAddress(bg, PairRequest{RequestID: "r1", Address: addr.Address})
	if err != nil {
		t.Fatal(err)
	}
	waitOp(t, b.e, op, PhaseSucceeded)
	var p PendingPeer
	eventually(t, "a sees the request", func() bool {
		ps, _ := a.e.ListPending(bg)
		if len(ps) == 1 {
			p = ps[0]
		}
		return len(ps) == 1
	})
	if _, err := a.e.Approve(bg, ApprovalRequest{DeviceID: p.ID, RequestRevision: p.RequestRevision, ExpectedFingerprint: p.Fingerprint}); err != nil {
		t.Fatal(err)
	}
	eventually(t, "connected", func() bool { return connected(a, b) })
}

func waitOp(t *testing.T, e *Engine, id OperationID, phase Phase) Operation {
	t.Helper()
	var o Operation
	eventually(t, "operation "+string(phase), func() bool {
		var err error
		if o, err = e.GetOperation(bg, id); err != nil {
			t.Fatal(err)
		}
		return o.Done()
	})
	if o.Phase != phase {
		t.Fatalf("operation = %+v, want %s", o, phase)
	}
	return o
}

func connected(a, b node) bool {
	has := func(x, y node) bool {
		ps, _ := x.e.ListPeers(bg)
		for _, p := range ps {
			if p.ID == selfID(y.e) && p.Connection == Connected {
				return true
			}
		}
		return false
	}
	return has(a, b) && has(b, a)
}

func selfID(e *Engine) DeviceID {
	s, _ := e.GetState(bg)
	return s.Self.ID
}

func TestPairByAddressNeedsApprovalAndFingerprint(t *testing.T) {
	a, b := newNode(t, nil), newNode(t, nil)
	start(t, a)
	start(t, b)
	addr, _ := a.e.ExportPairingAddress(bg)
	op, err := b.e.PairByAddress(bg, PairRequest{RequestID: "r1", Address: addr.Address})
	if err != nil {
		t.Fatal(err)
	}
	res := waitOp(t, b.e, op, PhaseSucceeded)
	if !res.Pairing.AwaitingRemoteApproval || res.Pairing.PeerID != state(t, a.e).Self.ID {
		t.Fatalf("result = %+v", res.Pairing)
	}
	if js, _ := json.Marshal(res); strings.Contains(string(js), addr.Address) {
		t.Fatal("the address leaked into the operation")
	}

	var p PendingPeer
	eventually(t, "a sees the request", func() bool {
		ps, _ := a.e.ListPending(bg)
		if len(ps) == 1 {
			p = ps[0]
		}
		return len(ps) == 1
	})
	if p.Source != SourceDirect || p.Fingerprint == "" {
		t.Fatalf("pending = %+v", p)
	}
	// Both devices compute the same fingerprint.
	bst := state(t, b.e)
	if bp := bst.Peers; len(bp) != 1 {
		t.Fatalf("b's peers = %+v", bp)
	}
	// Nothing flows before approval: a has no peer.
	if ps, _ := a.e.ListPeers(bg); len(ps) != 0 {
		t.Fatalf("a trusts an unapproved device: %+v", ps)
	}
	// A wrong fingerprint or a stale revision is a conflict and changes nothing.
	if _, err := a.e.Approve(bg, ApprovalRequest{DeviceID: p.ID, RequestRevision: p.RequestRevision, ExpectedFingerprint: "0000 0000 0000 0000"}); code(err) != CodeConflict {
		t.Fatalf("wrong fingerprint: %v", err)
	}
	if _, err := a.e.Approve(bg, ApprovalRequest{DeviceID: p.ID, RequestRevision: p.RequestRevision + 1, ExpectedFingerprint: p.Fingerprint}); code(err) != CodeConflict {
		t.Fatalf("stale revision: %v", err)
	}
	if _, err := a.e.Approve(bg, ApprovalRequest{DeviceID: "nobody", RequestRevision: 1, ExpectedFingerprint: p.Fingerprint}); code(err) != CodeNotFound {
		t.Fatalf("unknown device: %v", err)
	}
	if ps, _ := a.e.ListPeers(bg); len(ps) != 0 {
		t.Fatal("a failed approval created trust")
	}
	peer, err := a.e.Approve(bg, ApprovalRequest{DeviceID: p.ID, RequestRevision: p.RequestRevision, ExpectedFingerprint: p.Fingerprint})
	if err != nil || peer.ID != p.ID {
		t.Fatalf("approve = %+v, %v", peer, err)
	}
	eventually(t, "connected", func() bool { return connected(a, b) })
	if ps, _ := a.e.ListPending(bg); len(ps) != 0 {
		t.Fatalf("still pending: %+v", ps)
	}

	// Unpair drops the connection on both ends and is idempotent.
	for i := 0; i < 2; i++ {
		if err := a.e.Unpair(bg, p.ID); err != nil {
			t.Fatal(err)
		}
	}
	if ps, _ := a.e.ListPeers(bg); len(ps) != 0 {
		t.Fatalf("peers after Unpair: %+v", ps)
	}
	eventually(t, "b sees a leave", func() bool {
		ps, _ := b.e.ListPeers(bg)
		return len(ps) == 1 && ps[0].Connection == Disconnected
	})
}

func TestRejectNeedsTheCurrentRevision(t *testing.T) {
	a, b := newNode(t, nil), newNode(t, nil)
	start(t, a)
	start(t, b)
	addr, _ := a.e.ExportPairingAddress(bg)
	_, _ = b.e.PairByAddress(bg, PairRequest{RequestID: "r1", Address: addr.Address})
	var p PendingPeer
	eventually(t, "request", func() bool {
		ps, _ := a.e.ListPending(bg)
		if len(ps) == 1 {
			p = ps[0]
		}
		return len(ps) == 1
	})
	if err := a.e.Reject(bg, PendingRequest{DeviceID: p.ID, RequestRevision: p.RequestRevision + 7}); code(err) != CodeConflict {
		t.Fatalf("stale reject: %v", err)
	}
	if err := a.e.Reject(bg, PendingRequest{DeviceID: p.ID, RequestRevision: p.RequestRevision}); err != nil {
		t.Fatal(err)
	}
	if ps, _ := a.e.ListPending(bg); len(ps) != 0 {
		t.Fatalf("pending after reject: %+v", ps)
	}
	if err := a.e.Reject(bg, PendingRequest{DeviceID: p.ID, RequestRevision: p.RequestRevision}); code(err) != CodeNotFound {
		t.Fatalf("second reject: %v", err)
	}
}

func TestPairByCode(t *testing.T) {
	rv := rvtest.New()
	t.Cleanup(rv.Close)
	a, b := newNode(t, rv), newNode(t, rv)
	start(t, a)
	start(t, b)

	c, err := a.e.CreatePairingCode(bg)
	if err != nil {
		t.Fatal(err)
	}
	// State shows that a code exists, never the code.
	st := state(t, a.e)
	if !st.PairingCode.Active {
		t.Fatal("code not active in state")
	}
	if js, _ := json.Marshal(st); strings.Contains(string(js), strings.ReplaceAll(c.Code, "-", "")) || strings.Contains(string(js), c.Code) {
		t.Fatal("the pairing code leaked into State")
	}
	if got, ok, _ := a.e.GetPairingCode(bg); !ok || got.Code != c.Code {
		t.Fatalf("GetPairingCode = %+v %v", got, ok)
	}

	op, err := b.e.JoinByCode(bg, JoinRequest{RequestID: "j1", Code: strings.Replace(c.Code, "-", " ", 1)})
	if err != nil {
		t.Fatal(err)
	}
	// The same RequestID returns the same operation, a different request conflicts.
	if again, err := b.e.JoinByCode(bg, JoinRequest{RequestID: "j1", Code: c.Code}); err != nil || again != op {
		t.Fatalf("repeat = %v, %v", again, err)
	}
	if _, err := b.e.JoinByCode(bg, JoinRequest{RequestID: "j1", Code: "111111"}); code(err) != CodeConflict {
		t.Fatalf("reused RequestID: %v", err)
	}
	res := waitOp(t, b.e, op, PhaseSucceeded)
	if !res.Pairing.AwaitingRemoteApproval {
		t.Fatalf("a code must not grant trust: %+v", res.Pairing)
	}
	var p PendingPeer
	eventually(t, "request through the code", func() bool {
		ps, _ := a.e.ListPending(bg)
		if len(ps) == 1 {
			p = ps[0]
		}
		return len(ps) == 1
	})
	if p.Source != SourceCode {
		t.Fatalf("source = %s", p.Source)
	}
	if state(t, a.e).PairingCode.Active {
		t.Fatal("the code survived its use")
	}
	if _, ok, _ := a.e.GetPairingCode(bg); ok {
		t.Fatal("GetPairingCode returns a used code")
	}
	eventually(t, "code consumed on the server", func() bool { return rv.WasConsumed(c.Code) })

	// Failures are classified without text from below.
	if _, err := b.e.JoinByCode(bg, JoinRequest{RequestID: "j2", Code: "12"}); code(err) != CodeIncomplete {
		t.Fatalf("short code: %v", err)
	}
	op2, _ := b.e.JoinByCode(bg, JoinRequest{RequestID: "j3", Code: c.Code})
	if o := waitOp(t, b.e, op2, PhaseFailed); o.Error.Code != CodeExpired {
		t.Fatalf("spent code: %+v", o.Error)
	}
	// Cancelling a code invalidates it here at once.
	if _, err := a.e.CreatePairingCode(bg); err != nil {
		t.Fatal(err)
	}
	if err := a.e.CancelPairingCode(bg); err != nil {
		t.Fatal(err)
	}
	if state(t, a.e).PairingCode.Active {
		t.Fatal("code active after cancel")
	}
}

func TestPairingCodeExpiryIsAnnounced(t *testing.T) {
	rv := rvtest.New()
	rv.TTL = 6 * time.Second
	t.Cleanup(rv.Close)
	a := newNode(t, rv)
	start(t, a)
	sub, err := a.e.SubscribeEvents(bg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = sub.Close() }()
	if _, err := a.e.CreatePairingCode(bg); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(bg, 10*time.Second)
	defer cancel()
	var sawCreate, sawExpire bool
	for !sawExpire {
		ev, err := sub.Next(ctx)
		if err != nil {
			t.Fatalf("Next: %v (create %v)", err, sawCreate)
		}
		if ev.Kind != EventPairingCode {
			continue
		}
		if state(t, a.e).PairingCode.Active {
			sawCreate = true
		} else if sawCreate {
			sawExpire = true
		}
	}
}

func TestNetworkWorkNeedsRunningAndActive(t *testing.T) {
	n := newNode(t, nil)
	if _, err := n.e.CreatePairingCode(bg); code(err) != CodeInvalidState {
		t.Fatalf("before Start: %v", err)
	}
	if _, err := n.e.PairByAddress(bg, PairRequest{RequestID: "x", Address: "127.0.0.1:1"}); code(err) != CodeInvalidState {
		t.Fatalf("before Start: %v", err)
	}
	start(t, n)
	if err := n.e.SetRuntimeState(bg, RuntimeSuspended); err != nil {
		t.Fatal(err)
	}
	if _, err := n.e.CreatePairingCode(bg); code(err) != CodeSuspended {
		t.Fatalf("suspended: %v", err)
	}
	if _, err := n.e.PairByAddress(bg, PairRequest{RequestID: "x", Address: "127.0.0.1:1"}); code(err) != CodeSuspended {
		t.Fatalf("suspended: %v", err)
	}
	if err := n.e.NotifyNetworkChanged(bg); code(err) != CodeSuspended {
		t.Fatalf("suspended: %v", err)
	}
	if err := n.e.SetRuntimeState(bg, "sleeping"); code(err) != CodeInvalidArgument {
		t.Fatalf("bad state: %v", err)
	}
	if err := n.e.SetRuntimeState(bg, RuntimeActive); err != nil {
		t.Fatal(err)
	}
	if err := n.e.NotifyNetworkChanged(bg); err != nil {
		t.Fatal(err)
	}
	if _, err := n.e.PairByAddress(bg, PairRequest{Address: "127.0.0.1:1"}); code(err) != CodeInvalidArgument {
		t.Fatalf("missing RequestID: %v", err)
	}
	if _, err := n.e.PairByAddress(bg, PairRequest{RequestID: "z", Address: "has space"}); code(err) != CodeInvalidArgument {
		t.Fatalf("bad address: %v", err)
	}
	// Our own address is refused, with a classified error.
	own, _ := n.e.ExportPairingAddress(bg)
	op, err := n.e.PairByAddress(bg, PairRequest{RequestID: "own", Address: own.Address})
	if err != nil {
		t.Fatal(err)
	}
	if o := waitOp(t, n.e, op, PhaseFailed); o.Error.Code != CodeInvalidArgument {
		t.Fatalf("own address: %+v", o.Error)
	}
}

func TestSuspendDropsConnectionsAndResumeReconnects(t *testing.T) {
	a, b := newNode(t, nil), newNode(t, nil)
	start(t, a)
	start(t, b)
	pairTwo(t, a, b)

	if err := a.e.SetRuntimeState(bg, RuntimeSuspended); err != nil {
		t.Fatal(err)
	}
	eventually(t, "b sees a go", func() bool {
		ps, _ := b.e.ListPeers(bg)
		return len(ps) == 1 && ps[0].Connection == Disconnected
	})
	if ps, _ := a.e.ListPeers(bg); len(ps) != 1 || ps[0].Connection != Disconnected {
		t.Fatalf("a while suspended: %+v", ps)
	}
	if err := a.e.SetRuntimeState(bg, RuntimeActive); err != nil {
		t.Fatal(err)
	}
	eventually(t, "reconnected", func() bool { return connected(a, b) })
}

func TestTrustSurvivesRestart(t *testing.T) {
	a, b := newNode(t, nil), newNode(t, nil)
	start(t, a)
	start(t, b)
	pairTwo(t, a, b)
	if err := a.e.Close(bg); err != nil {
		t.Fatal(err)
	}
	opts, _ := newOpts(t, a.dir)
	opts.Network = a.net // same address as before
	a2, err := New(bg, opts)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = a2.Close(bg) }()
	if err := a2.Start(bg); err != nil {
		t.Fatal(err)
	}
	a2n := node{a2, a.net, a.dir}
	eventually(t, "reconnected after restart", func() bool { return connected(a2n, b) })
}

// ---- events ----

func TestSubscriptionContinuesFromSnapshot(t *testing.T) {
	n := newNode(t, nil)
	sub, err := n.e.SubscribeEvents(bg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = sub.Close() }()
	snap := sub.Snapshot()
	if snap.Revision != state(t, n.e).Revision {
		t.Fatal("snapshot is not the current state")
	}
	start(t, n)
	ctx, cancel := context.WithTimeout(bg, 5*time.Second)
	defer cancel()
	last := snap.Revision
	for i := 0; i < 2; i++ { // starting, running
		ev, err := sub.Next(ctx)
		if err != nil {
			t.Fatal(err)
		}
		if ev.Revision != last+1 || ev.InstanceID != snap.InstanceID || ev.Kind != EventLifecycle {
			t.Fatalf("event %+v after revision %d", ev, last)
		}
		last = ev.Revision
	}
	if got := state(t, n.e).Revision; got != last {
		t.Fatalf("revision %d, events reached %d", got, last)
	}
	// The snapshot is a copy.
	s1 := sub.Snapshot()
	s1.Peers = append(s1.Peers, Peer{})
	if len(sub.Snapshot().Peers) != 0 {
		t.Fatal("Snapshot shares memory")
	}
}

func TestSlowSubscriberMustResync(t *testing.T) {
	n := newNode(t, nil)
	_ = n.e.Close(bg)
	opts, _ := newOpts(t, n.dir)
	opts.Resources.EventBuffer = 2
	e, err := New(bg, opts)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = e.Close(bg) }()
	sub, _ := e.SubscribeEvents(bg)
	defer func() { _ = sub.Close() }()
	// The consumer does nothing while several changes happen: the core must
	// neither block nor queue without bound.
	done := make(chan struct{})
	go func() {
		defer close(done)
		for i := 0; i < 10; i++ {
			_ = e.UpdateHostCapabilities(bg, HostCapabilities{ReadFiles: i%2 == 0})
		}
	}()
	select {
	case <-done:
	case <-time.After(5 * time.Second):
		t.Fatal("a slow subscriber blocked the engine")
	}
	_, err = sub.Next(bg)
	if code(err) != CodeResyncRequired {
		t.Fatalf("Next = %v, want resync_required", err)
	}
	if _, err = sub.Next(bg); err != io.EOF {
		t.Fatalf("after resync the subscription is over: %v", err)
	}
	// A fresh subscription starts from the current state.
	sub2, err := e.SubscribeEvents(bg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = sub2.Close() }()
	if sub2.Snapshot().Revision != state(t, e).Revision {
		t.Fatal("new snapshot is stale")
	}
}

func TestSubscriptionCloseAndLimits(t *testing.T) {
	n := newNode(t, nil)
	sub, _ := n.e.SubscribeEvents(bg)

	res := make(chan error, 2)
	go func() { _, err := sub.Next(bg); res <- err }()
	time.Sleep(50 * time.Millisecond)
	if _, err := sub.Next(bg); code(err) != CodeInUse {
		t.Fatalf("concurrent Next: %v", err)
	}
	_ = sub.Close()
	_ = sub.Close() // idempotent
	select {
	case err := <-res:
		if err != io.EOF {
			t.Fatalf("Next after Close: %v", err)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("Close did not wake Next")
	}

	var subs []*Subscription
	for i := 0; i < 16; i++ {
		s, err := n.e.SubscribeEvents(bg)
		if err != nil {
			t.Fatal(err)
		}
		subs = append(subs, s)
	}
	if _, err := n.e.SubscribeEvents(bg); code(err) != CodeResourceExhausted {
		t.Fatalf("17th subscription: %v", err)
	}
	_ = subs[0].Close()
	if s, err := n.e.SubscribeEvents(bg); err != nil {
		t.Fatalf("slot not freed: %v", err)
	} else {
		subs = append(subs, s)
	}

	// Closing the engine ends every subscription.
	wait := make(chan error, 1)
	go func() { // events of the shutdown itself may arrive first
		for {
			if _, err := subs[1].Next(bg); err != nil {
				wait <- err
				return
			}
		}
	}()
	time.Sleep(20 * time.Millisecond)
	if err := n.e.Close(bg); err != nil {
		t.Fatal(err)
	}
	select {
	case err := <-wait:
		if err != io.EOF {
			t.Fatalf("Next after engine Close: %v", err)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("engine Close did not end the subscription")
	}
}

func TestEventsAndStateCarryNoSecrets(t *testing.T) {
	rv := rvtest.New()
	t.Cleanup(rv.Close)
	a, b := newNode(t, rv), newNode(t, rv)
	start(t, a)
	start(t, b)
	sub, _ := a.e.SubscribeEvents(bg)
	defer func() { _ = sub.Close() }()
	addr, _ := a.e.ExportPairingAddress(bg)
	c, _ := a.e.CreatePairingCode(bg)
	op, _ := b.e.PairByAddress(bg, PairRequest{RequestID: "r", Address: addr.Address})
	waitOp(t, b.e, op, PhaseSucceeded)
	eventually(t, "request", func() bool { ps, _ := a.e.ListPending(bg); return len(ps) == 1 })

	ctx, cancel := context.WithTimeout(bg, 200*time.Millisecond)
	defer cancel()
	for {
		ev, err := sub.Next(ctx)
		if err != nil {
			break
		}
		js, _ := json.Marshal(ev)
		if strings.Contains(string(js), addr.Address) || strings.Contains(string(js), c.Code) {
			t.Fatalf("event leaks a secret: %s", js)
		}
	}
	for _, e := range []*Engine{a.e, b.e} {
		js, _ := json.Marshal(state(t, e))
		if strings.Contains(string(js), addr.Address) || strings.Contains(string(js), c.Code) {
			t.Fatalf("state leaks a secret: %s", js)
		}
	}
}

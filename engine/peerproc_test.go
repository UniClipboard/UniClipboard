package engine

import (
	"context"
	"errors"
	"strings"
	"sync"
	"sync/atomic"
	"testing"

	"github.com/uniclipboard/unic/internal/store"
	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/plugin/rpc"
)

// fakePeer is an in-process peer_sessions backend that records its input.
type fakePeer struct {
	storeBackend
	auth     plugin.PeerAuthority
	pmu      sync.Mutex
	binds    []rpc.Bind
	states   []rpc.SyncState
	reject   error
	recv     plugin.PeerReceiver
	peer     *fakePeer
	peerConn string
	peerTok  string
	sends    atomic.Int64
	lastErr  error
	lastDec  string
	hold     chan struct{}
	purposes []string
	dialed   rpc.Connect
	partner  *fakePeer
	tt       *testing.T
	connErr  error
}

func (f *fakePeer) Spec() plugin.Spec {
	return plugin.Spec{ID: "test.peer", Version: "1", APIVersion: 1, Capabilities: []string{plugin.CapSyncBackend}, TransportKind: plugin.TransportPeerSessions}
}
func (f *fakePeer) Bind(_ context.Context, b rpc.Bind) error {
	f.pmu.Lock()
	defer f.pmu.Unlock()
	f.binds = append(f.binds, b)
	return nil
}
func (f *fakePeer) ApplyState(_ context.Context, s rpc.SyncState) error {
	f.pmu.Lock()
	defer f.pmu.Unlock()
	if f.reject != nil {
		return f.reject
	}
	f.states = append(f.states, s)
	return nil
}
func (f *fakePeer) last() (rpc.SyncState, int) {
	f.pmu.Lock()
	defer f.pmu.Unlock()
	if len(f.states) == 0 {
		return rpc.SyncState{}, 0
	}
	return f.states[len(f.states)-1], len(f.states)
}

func peerNode(t *testing.T) (node, *fakePeer) {
	fp := &fakePeer{}
	n := newContentNode(t, func(o *Options) { o.Plugins = []PluginInstance{{Plugin: fp}} })
	return n, fp
}

func TestPeerStateSnapshotAndBarrier(t *testing.T) {
	n, fp := peerNode(t)
	start(t, n)
	if len(fp.binds) != 1 || fp.binds[0].BackendID != "test.peer" {
		t.Fatalf("bind: %+v", fp.binds)
	}
	s, cnt := fp.last()
	// One push when the plugin loads (suspended), one when the network comes up.
	if cnt != 2 || s.Revision != 2 || s.Runtime != "active" || len(s.Peers) != 0 || !s.SyncEnabled {
		t.Fatalf("initial state: %+v (%d)", s, cnt)
	}
	// An unchanged snapshot is not pushed again.
	n.e.syncPeerStates()
	if _, c := fp.last(); c != 2 {
		t.Fatalf("redundant push: %d", c)
	}

	d, err := n.e.st.AddDevice("dev1", make([]byte, 32), "Laptop")
	if err != nil {
		t.Fatal(err)
	}
	if d, err = n.e.st.TrustDevice(d.ID, d.Revision); err != nil {
		t.Fatal(err)
	}
	b, err := n.e.st.AddBinding("b1", d.ID, "test.peer", []byte{})
	if err != nil {
		t.Fatal(err)
	}
	if _, err = n.e.st.AuthorizeBinding(b.ID, b.Revision); err != nil {
		t.Fatal(err)
	}
	if err = n.e.st.SetBindingEndpoint(b.ID, []byte("addr-1"), false); err != nil {
		t.Fatal(err)
	}
	n.e.syncPeerStates()
	s, cnt = fp.last()
	if cnt != 3 || s.Revision != 3 || len(s.Peers) != 1 || s.Peers[0].ID != "dev1" || s.Peers[0].Address != "addr-1" {
		t.Fatalf("after authorize: %+v (%d)", s, cnt)
	}

	// Unpair returns only after the backend has the new state.
	cur, _ := n.e.st.Device("dev1")
	if err := n.e.UnpairDevice(bg, UnpairDeviceRequest{ID: "dev1", ExpectedRevision: cur.Revision}); err != nil {
		t.Fatal(err)
	}
	s, cnt = fp.last()
	if cnt != 4 || len(s.Peers) != 0 || len(s.Rejected) != 1 || s.Rejected[0].ID != "dev1" {
		t.Fatalf("after unpair: %+v (%d)", s, cnt)
	}

	// Suspend and network change each produce a new state.
	_ = n.e.SetRuntimeState(bg, RuntimeSuspended)
	if s, _ = fp.last(); s.Runtime != "suspended" {
		t.Fatalf("runtime: %q", s.Runtime)
	}
	_ = n.e.SetRuntimeState(bg, RuntimeActive)
	_, before := fp.last()
	_ = n.e.NotifyNetworkChanged(bg)
	if s, c := fp.last(); c != before+1 || s.NetworkGeneration == 0 {
		t.Fatalf("network: %+v (%d)", s, c)
	}
	var _ = store.TrustRevoked
}

func TestPeerStateRejectedKeepsLast(t *testing.T) {
	n, fp := peerNode(t)
	fp.reject = rpc.NewError(rpc.CodeInvalidArgument)
	n.e.mu.Lock()
	cur := n.e.settings
	n.e.mu.Unlock()
	v := cur.Value
	v.DeviceName = "Renamed"
	_, _ = n.e.UpdateSettings(bg, UpdateSettingsRequest{ExpectedRevision: cur.Revision, Value: v})
	if _, c := fp.last(); c != 1 {
		t.Fatalf("rejected state counted: %d", c)
	}
	fp.reject = nil
	n.e.syncPeerStates()
	if s, c := fp.last(); c != 2 || s.Revision != 2 || s.DeviceName != "Renamed" {
		t.Fatalf("retry: %+v (%d)", s, c)
	}
}

func (f *fakePeer) SetPeerAuthority(a plugin.PeerAuthority) { f.auth = a }

// runHandshake runs one connection between a (outgoing) and b (incoming) the way
// two backends would, carrying bytes only. bindB lets a test break the binding.
func runHandshake(t *testing.T, a, b *fakePeer, conn string, attA *string, bindA, bindB []byte) (da rpc.PeerDecision, ea error, db rpc.PeerDecision, eb error) {
	t.Helper()
	b64s := func(x []byte) string { return b64.EncodeToString(x) }
	ha, err := a.auth.IdentityHello(bg, rpc.IdentityHello{ConnectionID: conn, Direction: "outgoing", AttemptID: attA, ContextBinding: b64s(bindA)})
	if err != nil {
		return da, err, db, nil
	}
	hb, err := b.auth.IdentityHello(bg, rpc.IdentityHello{ConnectionID: conn, Direction: "incoming", ContextBinding: b64s(bindB)})
	if err != nil {
		return da, nil, db, err
	}
	pa, err := a.auth.IdentityProve(bg, rpc.IdentityProve{ConnectionID: conn, PeerHello: hb.Hello})
	if err != nil {
		return da, err, db, nil
	}
	pb, err := b.auth.IdentityProve(bg, rpc.IdentityProve{ConnectionID: conn, PeerHello: ha.Hello})
	if err != nil {
		return da, nil, db, err
	}
	wire := rpc.PeerWire{WireVersion: 1, Name: "peer", Address: "addr-x"}
	da, ea = a.auth.CheckPeer(bg, rpc.CheckPeer{ConnectionID: conn, Direction: "outgoing", AttemptID: attA, Hello: wire, PeerProof: pb.Proof, PeerPairProof: pb.PairProof})
	db, eb = b.auth.CheckPeer(bg, rpc.CheckPeer{ConnectionID: conn, Direction: "incoming", Hello: wire, PeerProof: pa.Proof, PeerPairProof: pa.PairProof})
	return
}

func errCode(err error) string {
	var re *rpc.Error
	if errors.As(err, &re) {
		return re.Code
	}
	if err == nil {
		return ""
	}
	return "?"
}

func TestPeerHandshake(t *testing.T) {
	na, fa := peerNode(t)
	nb, fb := peerNode(t)
	bind := []byte("conn-binding-1")

	// An unknown device is recorded and stays pending on both sides.
	da, ea, db, eb := runHandshake(t, fa, fb, "c1", nil, bind, bind)
	if ea != nil || eb != nil || da.Decision != "pending" || db.Decision != "pending" || da.SessionToken != nil {
		t.Fatalf("first contact: %+v %v / %+v %v", da, ea, db, eb)
	}
	idA, idB := na.e.ident.DeviceID(), nb.e.ident.DeviceID()
	if da.PeerID != idB || db.PeerID != idA {
		t.Fatalf("peer ids come from the proven keys: %q %q", da.PeerID, db.PeerID)
	}
	if d, err := nb.e.st.Device(idA); err != nil || d.Trust != store.TrustPending {
		t.Fatalf("recorded as pending: %+v %v", d, err)
	}

	// Once both users approved, the decision is allow with a token.
	approve := func(n node, peer string) {
		d, _ := n.e.st.Device(peer)
		d, _ = n.e.st.TrustDevice(peer, d.Revision)
		bd, err := n.e.st.InstanceBinding(peer, "test.peer")
		if err != nil {
			t.Fatal(err)
		}
		if _, err := n.e.st.AuthorizeBinding(bd.ID, bd.Revision); err != nil {
			t.Fatal(err)
		}
	}
	approve(na, idB)
	approve(nb, idA)
	da, ea, db, eb = runHandshake(t, fa, fb, "c2", nil, bind, bind)
	if ea != nil || eb != nil || da.Decision != "allow" || db.Decision != "allow" || da.SessionToken == nil || len(*da.SessionToken) != 64 {
		t.Fatalf("approved: %+v %v / %+v %v", da, ea, db, eb)
	}

	// A different context binding invalidates the proofs.
	_, ea, _, eb = runHandshake(t, fa, fb, "c3", nil, bind, []byte("other"))
	if errCode(ea) != "bad_proof" || errCode(eb) != "bad_proof" {
		t.Fatalf("binding: %q %q", errCode(ea), errCode(eb))
	}
	// A connection ID cannot be reused while its session lives.
	if _, ea, _, _ = runHandshake(t, fa, fb, "c2", nil, bind, bind); errCode(ea) != "conflict" {
		t.Fatalf("reuse: %q", errCode(ea))
	}
}

func TestPeerHandshakeLimitsAndAttempts(t *testing.T) {
	_, fa := peerNode(t)
	nb, fb := peerNode(t)
	nc, fc := peerNode(t) // answers attempts; na is left full below
	bind := b64.EncodeToString([]byte("x"))
	for i := 0; i < maxHandshakes; i++ {
		if _, err := fa.auth.IdentityHello(bg, rpc.IdentityHello{ConnectionID: "h" + string(rune('a'+i)), Direction: "incoming", ContextBinding: bind}); err != nil {
			t.Fatal(err)
		}
	}
	if _, err := fa.auth.IdentityHello(bg, rpc.IdentityHello{ConnectionID: "zz", Direction: "incoming", ContextBinding: bind}); errCode(err) != "resource_exhausted" {
		t.Fatalf("limit: %q", errCode(err))
	}
	// Unknown connection, unknown attempt.
	if _, err := fb.auth.IdentityProve(bg, rpc.IdentityProve{ConnectionID: "nope", PeerHello: b64.EncodeToString(make([]byte, 66))}); errCode(err) != "not_found" {
		t.Fatalf("unknown conn: %q", errCode(err))
	}
	bogus := randHex(32)
	if _, err := fb.auth.IdentityHello(bg, rpc.IdentityHello{ConnectionID: "q", Direction: "outgoing", AttemptID: &bogus, ContextBinding: bind}); errCode(err) != "not_found" {
		t.Fatalf("unknown attempt: %q", errCode(err))
	}

	// An attempt that expects another peer fails with identity_mismatch and is used up.
	pb := nb.e.peerProcs[0]
	id, err := pb.newAttempt("somewhere", "not-the-peer", nil)
	if err != nil {
		t.Fatal(err)
	}
	_, ea, _, _ := runHandshake(t, fb, fc, "c9", &id, []byte("b"), []byte("b"))
	if errCode(ea) != "identity_mismatch" {
		t.Fatalf("expected peer: %q", errCode(ea))
	}
	if _, err := fb.auth.IdentityHello(bg, rpc.IdentityHello{ConnectionID: "c10", Direction: "outgoing", AttemptID: &id, ContextBinding: bind}); errCode(err) != "not_found" {
		t.Fatalf("used attempt: %q", errCode(err))
	}

	// With the right expectation and a pairing token the incoming side sees a
	// pair proof it cannot match to a live ticket.
	id, _ = pb.newAttempt("somewhere", nc.e.ident.DeviceID(), []byte("0123456789abcdef"))
	_, _, _, eb := runHandshake(t, fb, fc, "c11", &id, []byte("b"), []byte("b"))
	if errCode(eb) != "bad_pair_proof" {
		t.Fatalf("stale ticket: %q", errCode(eb))
	}
}

// allowedPair gives a (which approves b) and b, after a first contact.
func allowedPair(t *testing.T) (na, nb node, fa, fb *fakePeer) {
	na, fa = peerNode(t)
	nb, fb = peerNode(t)
	bind := []byte("first")
	if _, ea, _, eb := runHandshake(t, fa, fb, "first", nil, bind, bind); ea != nil || eb != nil {
		t.Fatal(ea, eb)
	}
	idB := nb.e.ident.DeviceID()
	d, _ := na.e.st.Device(idB)
	if _, err := na.e.st.TrustDevice(idB, d.Revision); err != nil {
		t.Fatal(err)
	}
	bd, err := na.e.st.InstanceBinding(idB, "test.peer")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := na.e.st.AuthorizeBinding(bd.ID, bd.Revision); err != nil {
		t.Fatal(err)
	}
	na.e.syncPeerStates()
	return
}

func allowConn(t *testing.T, fa, fb *fakePeer, conn string, outgoing bool) string {
	t.Helper()
	bind := []byte(conn)
	var d rpc.PeerDecision
	var err error
	if outgoing {
		d, err, _, _ = runHandshake(t, fa, fb, conn, nil, bind, bind)
	} else {
		_, _, d, err = runHandshake(t, fb, fa, conn, nil, bind, bind)
	}
	if err != nil || d.Decision != "allow" {
		t.Fatalf("%s: %+v %v", conn, d, err)
	}
	return *d.SessionToken
}

func TestPeerConnectionEvents(t *testing.T) {
	na, _, fa, fb := allowedPair(t)
	idB := fb.auth.(*peerProcBackend).e.ident.DeviceID()
	pa := na.e.peerProcs[0]
	ev := func(conn, tok string, seq uint64, state string) error {
		return fa.auth.PeerEvent(bg, rpc.PeerEvent{ConnectionID: conn, SessionToken: tok, Sequence: rpc.U64(seq), State: state, RemoteAccepted: state == "connected"})
	}
	tok := allowConn(t, fa, fb, "e1", true)
	if err := ev("e1", tok, 1, "connected"); err != nil || !pa.connected(idB) {
		t.Fatalf("connected: %v", err)
	}
	if ds, _ := na.e.ListDevices(bg); len(ds) == 0 || ds[0].Bindings[0].Connection != Connected {
		t.Fatalf("listed as connected: %+v", ds)
	}
	if err := ev("e1", tok, 1, "connected"); err != nil {
		t.Fatalf("idempotent: %v", err)
	}
	if err := ev("e1", tok, 1, "disconnected"); errCode(err) != "conflict" {
		t.Fatalf("same sequence, other state: %q", errCode(err))
	}
	if err := ev("e1", strings.Repeat("0", 64), 2, "disconnected"); errCode(err) != "permission_denied" {
		t.Fatalf("token: %q", errCode(err))
	}
	if err := ev("nope", tok, 2, "disconnected"); errCode(err) != "not_found" {
		t.Fatalf("unknown connection: %q", errCode(err))
	}
	if err := ev("e1", tok, 3, "disconnected"); err != nil || pa.connected(idB) {
		t.Fatalf("disconnected: %v", err)
	}
	if err := ev("e1", tok, 2, "connected"); err != nil || pa.connected(idB) {
		t.Fatalf("a stale sequence changes nothing: %v", err)
	}
	if err := ev("e1", tok, 4, "connected"); errCode(err) != "invalid_state" {
		t.Fatalf("a disconnected token stays dead: %q", errCode(err))
	}

	// Two simultaneous connections: the one dialed by the smaller ID survives.
	t1 := allowConn(t, fa, fb, "d1", true)  // dialed by us
	t2 := allowConn(t, fa, fb, "d2", false) // dialed by the peer
	self := na.e.ident.DeviceID()
	ourWins := self < idB
	if err := ev("d1", t1, 1, "connected"); err != nil {
		t.Fatal(err)
	}
	err := ev("d2", t2, 1, "connected")
	if ourWins && errCode(err) != "conflict" || !ourWins && err != nil {
		t.Fatalf("winner rule (ours wins=%v): %v", ourWins, err)
	}
	if !pa.connected(idB) {
		t.Fatal("one connection must survive")
	}
	if !ourWins { // d1 was replaced; its late report cannot flip the state
		if err := ev("d1", t1, 2, "disconnected"); err != nil || !pa.connected(idB) {
			t.Fatalf("replaced connection: %v", err)
		}
	}

	// Revoking the device ends its connection before Unpair returns.
	cur, _ := na.e.st.Device(idB)
	if err := na.e.UnpairDevice(bg, UnpairDeviceRequest{ID: idB, ExpectedRevision: cur.Revision}); err != nil {
		t.Fatal(err)
	}
	if pa.connected(idB) {
		t.Fatal("still connected after revocation")
	}
}

// ---- pairing through a peer_sessions backend ----

func (f *fakePeer) PairingAddress(_ context.Context, purpose string) (string, error) {
	f.pmu.Lock()
	f.purposes = append(f.purposes, purpose)
	f.pmu.Unlock()
	return "fake://host:1", nil
}

// Connect plays both ends of a dial to the partner backend.
func (f *fakePeer) Connect(_ context.Context, c rpc.Connect) (rpc.ConnectResult, error) {
	f.pmu.Lock()
	f.dialed = c
	cerr := f.connErr
	f.pmu.Unlock()
	if cerr != nil {
		return rpc.ConnectResult{}, cerr
	}
	d, ea, _, eb := runHandshake(f.tt, f, f.partner, "pair-"+c.AttemptID[:8], &c.AttemptID, []byte("p"), []byte("p"))
	if ea != nil {
		return rpc.ConnectResult{}, ea
	}
	if eb != nil {
		return rpc.ConnectResult{}, eb
	}
	return rpc.ConnectResult{PeerID: d.PeerID, AwaitingRemoteApproval: true}, nil
}

func pairNodes(t *testing.T) (na, nb node, fa, fb *fakePeer) {
	na, fa = peerNode(t)
	nb, fb = peerNode(t)
	fa.tt, fb.tt, fb.partner = t, t, fa
	start(t, na)
	start(t, nb)
	return
}

func TestPeerPairingThroughBackend(t *testing.T) {
	na, nb, fa, fb := pairNodes(t)
	tk, err := na.e.ExportPairingTicketVia(bg, "test.peer")
	if err != nil {
		t.Fatal(err)
	}
	if fa.purposes[0] != rpc.AddressExport {
		t.Fatalf("purpose %v", fa.purposes)
	}
	op, err := nb.e.JoinByTicket(bg, JoinTicketRequest{RequestID: "r1", Ticket: tk.Ticket})
	if err != nil {
		t.Fatal(err)
	}
	o := waitOp(t, nb.e, op, PhaseSucceeded)
	if o.Pairing == nil || string(o.Pairing.PeerID) != string(na.e.ident.DeviceID()) || !o.Pairing.AwaitingRemoteApproval || !o.Pairing.AwaitingLocalApproval {
		t.Fatalf("pairing result: %+v", o.Pairing)
	}
	if fb.dialed.Address != "fake://host:1" || fb.dialed.ExpectedPeerID != na.e.ident.DeviceID() {
		t.Fatalf("dial: %+v", fb.dialed)
	}
	// The issuer saw the pair proof but trusts nothing: the device is pending.
	d, err := na.e.st.Device(nb.e.ident.DeviceID())
	if err != nil || d.Trust == store.TrustTrusted {
		t.Fatalf("issuer side: %+v %v", d, err)
	}
}

func TestPeerPairingRouting(t *testing.T) {
	na, nb, _, fb := pairNodes(t)
	if _, err := na.e.CreatePairingCodeVia(bg, "nobody"); err == nil {
		t.Fatal("unknown backend issued a code")
	}
	tk, err := na.e.ExportPairingTicketVia(bg, "test.peer")
	if err != nil {
		t.Fatal(err)
	}
	// A ticket for a backend this device does not have is refused, not rerouted.
	nc := newContentNode(t, func(*Options) {})
	start(t, nc)
	op, err := nc.e.JoinByTicket(bg, JoinTicketRequest{RequestID: "r1", Ticket: tk.Ticket})
	if err == nil {
		o := waitOp(t, nc.e, op, PhaseFailed)
		if o.Error == nil || o.Error.Code != CodeUnsupported {
			t.Fatalf("no backend: %+v", o.Error)
		}
	} else if e, ok := err.(*Error); !ok || e.Code != CodeUnsupported {
		t.Fatal(err)
	}
	// A pair proof for a ticket issued on another backend is not honoured.
	if na.e.pairUsedBy("test.other", [32]byte{}, []byte("x")) {
		t.Fatal("wrong backend redeemed")
	}
	_ = nb
	_ = fb
}

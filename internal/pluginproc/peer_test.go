package pluginproc

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/plugin/rpc"
)

const testBinding = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

func peerSpecT() plugin.Spec {
	return plugin.Spec{ID: "example.peer", Version: "1.0.0", APIVersion: 1,
		Capabilities: []string{plugin.CapSyncBackend}, TransportKind: plugin.TransportPeerSessions}
}

// peerHelper logs what it was told to files in its private directory, so the
// parent can observe the plugin's view. A peer named "hang" makes ApplyState
// block forever, as a stuck backend would.
type peerHelper struct {
	dir string
	ph  plugin.PeerHost
}

func (h *peerHelper) Spec() plugin.Spec { return peerSpecT() }
func (h *peerHelper) Init(context.Context, plugin.Host, plugin.Config) error {
	h.dir = os.Getenv("UNIC_PLUGIN_DIR")
	return nil
}
func (h *peerHelper) Start(context.Context) error { return nil }
func (h *peerHelper) Close(context.Context) error { return nil }
func (h *peerHelper) log(line string) {
	f, err := os.OpenFile(filepath.Join(h.dir, fmt.Sprintf("log-%d", os.Getpid())), os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0o600)
	if err == nil {
		fmt.Fprintln(f, line)
		f.Close()
	}
}
func (h *peerHelper) Bind(_ context.Context, b rpc.Bind) error {
	h.log("bind " + b.BindingID[:4])
	return nil
}
func (h *peerHelper) ApplyState(_ context.Context, s rpc.SyncState) error {
	for _, p := range s.Peers {
		if p.Name == "reject" {
			return rpc.NewError(rpc.CodeInvalidArgument)
		}
		if p.Name == "hang" {
			time.Sleep(time.Hour)
		}
	}
	h.log(fmt.Sprintf("state %d peers=%d", s.Revision, len(s.Peers)))
	return nil
}

func peerExeSpec(t *testing.T) Spec {
	s := exeSpec(t, "peer")
	s.Expected = peerSpecT()
	return s
}

func testBind() rpc.Bind {
	return rpc.Bind{BindingID: testBinding, SelfID: "selfselfselfselfselfselfse", SelfName: "me", WireVersion: 2, BackendID: "example.peer", TransportKind: "peer_sessions"}
}

func testState(rev uint64, names ...string) rpc.SyncState {
	st := rpc.SyncState{BackendID: "example.peer", BindingID: testBinding, Revision: rpc.U64(rev), Runtime: "active", DeviceName: "me",
		SyncEnabled: true, Policy: rpc.BackendPolicy{Text: true}, Peers: []rpc.StatePeer{}, Rejected: []rpc.StateRejected{}}
	for i, n := range names {
		st.Peers = append(st.Peers, rpc.StatePeer{ID: fmt.Sprintf("peer%022d", i), Name: n, Address: "addr", TrustRevision: 1})
	}
	return st
}

// logs returns every plugin-process log joined in pid order of first line.
func logs(t *testing.T, dir string) []string {
	t.Helper()
	m, _ := filepath.Glob(filepath.Join(dir, "log-*"))
	var out []string
	for _, f := range m {
		b, _ := os.ReadFile(f)
		for _, l := range strings.Split(strings.TrimSpace(string(b)), "\n") {
			if l != "" {
				out = append(out, l)
			}
		}
	}
	return out
}

func waitLog(t *testing.T, dir, want string) {
	t.Helper()
	for i := 0; i < 200; i++ {
		for _, l := range logs(t, dir) {
			if l == want {
				return
			}
		}
		time.Sleep(25 * time.Millisecond)
	}
	t.Fatalf("log %q never appeared; have %v", want, logs(t, dir))
}

func TestPeerBindAndState(t *testing.T) {
	spec := peerExeSpec(t)
	p, _ := start(t, spec, Options{}, nil)
	pb := p.(plugin.PeerBackend)
	ctx := context.Background()
	if err := pb.ApplyState(ctx, testState(1)); err == nil {
		t.Fatal("state before bind must be refused")
	}
	if err := pb.Bind(ctx, testBind()); err != nil {
		t.Fatal(err)
	}
	if err := pb.Bind(ctx, testBind()); err != nil {
		t.Fatalf("rebinding the same session: %v", err)
	}
	other := testBind()
	other.SelfName = "x"
	if err := pb.Bind(ctx, other); err == nil {
		t.Fatal("conflicting bind accepted")
	}
	for _, rev := range []uint64{1, 2, 2} {
		if err := pb.ApplyState(ctx, testState(rev, "a")); err != nil {
			t.Fatal(err)
		}
	}
	if err := pb.ApplyState(ctx, testState(1, "a")); err == nil {
		t.Fatal("revision rollback accepted")
	}
	if err := pb.ApplyState(ctx, testState(2, "b")); err == nil {
		t.Fatal("same revision with different content accepted")
	}
	waitLog(t, spec.Dir, "state 2 peers=1")
}

func TestPeerStateReplayedAfterCrash(t *testing.T) {
	spec := peerExeSpec(t)
	p, _ := start(t, spec, Options{Backoff: []time.Duration{10 * time.Millisecond}}, nil)
	pb := p.(plugin.PeerBackend)
	ctx := context.Background()
	if err := pb.Bind(ctx, testBind()); err != nil {
		t.Fatal(err)
	}
	if err := pb.ApplyState(ctx, testState(3, "a", "b")); err != nil {
		t.Fatal(err)
	}
	// Kill the process from outside; the restart must receive bind + state
	// before anything can use the new session.
	proxyOf(p).terminate(proxyOf(p).current())
	eventually(t, func() bool {
		n := 0
		for _, l := range logs(t, spec.Dir) {
			if l == "state 3 peers=2" {
				n++
			}
		}
		return n == 2
	})
}

func TestPeerStuckApplyKillsAndReplaysNewest(t *testing.T) {
	spec := peerExeSpec(t)
	p, _ := start(t, spec, Options{ProcessTimeout: 300 * time.Millisecond, Backoff: []time.Duration{10 * time.Millisecond}}, nil)
	pb := p.(plugin.PeerBackend)
	ctx := context.Background()
	if err := pb.Bind(ctx, testBind()); err != nil {
		t.Fatal(err)
	}
	if err := pb.ApplyState(ctx, testState(1, "a")); err != nil {
		t.Fatal(err)
	}
	first := proxyOf(p).current()
	// The backend hangs on this snapshot: the barrier is satisfied by
	// reclaiming the process, and the restart gets this state, not the old one.
	if err := pb.ApplyState(ctx, testState(2, "hang")); err != nil {
		t.Fatalf("barrier by reclamation: %v", err)
	}
	select {
	case <-first.exited:
	default:
		t.Fatal("old process still alive after the barrier returned")
	}
}

type proxyAccess interface {
	current() *session
	terminate(*session)
}

func proxyOf(p plugin.Plugin) proxyAccess { return p.(proxyAccess) }

func eventually(t *testing.T, ok func() bool) {
	t.Helper()
	for i := 0; i < 200; i++ {
		if ok() {
			return
		}
		time.Sleep(25 * time.Millisecond)
	}
	t.Fatal("condition not reached")
}

func TestPeerRejectionKeepsProcessUnlessRevoking(t *testing.T) {
	spec := peerExeSpec(t)
	p, _ := start(t, spec, Options{Backoff: []time.Duration{10 * time.Millisecond}}, nil)
	pb := p.(plugin.PeerBackend)
	ctx := context.Background()
	if err := pb.Bind(ctx, testBind()); err != nil {
		t.Fatal(err)
	}
	if err := pb.ApplyState(ctx, testState(1, "a")); err != nil {
		t.Fatal(err)
	}
	first := proxyOf(p).current()
	// Purely additive and refused: report it, keep the process, forget the state.
	if err := pb.ApplyState(ctx, testState(2, "a", "reject")); err == nil {
		t.Fatal("refusal was swallowed")
	}
	if proxyOf(p).current() != first {
		t.Fatal("process was reclaimed for a harmless refusal")
	}
	if err := pb.ApplyState(ctx, testState(2, "a", "b")); err != nil {
		t.Fatalf("refused revision must not stick: %v", err)
	}
	// Refused but it drops peer "b": the old state is unsafe, so reclaim.
	if err := pb.ApplyState(ctx, testState(3, "reject")); err != nil {
		t.Fatalf("revoking refusal should be settled by reclamation: %v", err)
	}
	select {
	case <-first.exited:
	default:
		t.Fatal("old process still alive after a refused revocation")
	}
}

func (h *peerHelper) PairingAddress(_ context.Context, purpose string) (string, error) {
	h.log("addr " + purpose)
	return "helper://host:9", nil
}

func (h *peerHelper) Connect(_ context.Context, c rpc.Connect) (rpc.ConnectResult, error) {
	h.log("connect " + c.Address)
	if c.Address == "bad" {
		return rpc.ConnectResult{}, rpc.NewError(rpc.CodeUnavailable)
	}
	return rpc.ConnectResult{PeerID: string(c.ExpectedPeerID), AwaitingRemoteApproval: true}, nil
}

func TestPeerPairingMethods(t *testing.T) {
	spec := peerExeSpec(t)
	p, _ := start(t, spec, Options{}, nil)
	pp, ok := p.(plugin.PeerPairing)
	if !ok {
		t.Fatal("proxy does not offer pairing")
	}
	ctx := context.Background()
	addr, err := pp.PairingAddress(ctx, rpc.AddressPairingCode)
	if err != nil || addr != "helper://host:9" {
		t.Fatalf("address: %q %v", addr, err)
	}
	if _, err := pp.PairingAddress(ctx, "bogus"); err == nil {
		t.Fatal("unknown purpose accepted")
	}
	peer := "abcdefghijklmnopqrstuvwxyz"
	res, err := pp.Connect(ctx, rpc.Connect{AttemptID: strings.Repeat("a", 64), Address: "helper://x", ExpectedPeerID: peer})
	if err != nil || res.PeerID != peer || !res.AwaitingRemoteApproval {
		t.Fatalf("connect: %+v %v", res, err)
	}
	if _, err := pp.Connect(ctx, rpc.Connect{AttemptID: strings.Repeat("a", 64), Address: "bad", ExpectedPeerID: peer}); err == nil {
		t.Fatal("backend failure not reported")
	}
	waitLog(t, spec.Dir, "addr "+rpc.AddressPairingCode)
	waitLog(t, spec.Dir, "connect helper://x")
}

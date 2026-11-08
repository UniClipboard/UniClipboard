package engine

import (
	"testing"

	"github.com/uniclipboard/unic/plugin/rpc"
)

// candidateSetup returns C (na) with an approved, connected introducer B (nb)
// and a separate device A (nc) that C has never met.
func candidateSetup(t *testing.T) (na, nb, nc node, fa, fb, fc *fakePeer, conn, tok string) {
	na, nb, fa, fb = allowedPair(t)
	nc, fc = peerNode(t)
	start(t, na)
	start(t, nc)
	fa.tt, fa.partner = t, fc
	fc.tt = t
	conn = "intro1"
	tok = allowConn(t, fa, fb, conn, true)
	if err := fa.auth.PeerEvent(bg, rpc.PeerEvent{ConnectionID: conn, SessionToken: tok, Sequence: 1, State: "connected", RemoteAccepted: true}); err != nil {
		t.Fatal(err)
	}
	return
}

func TestIntroduceMakesCandidatesOnly(t *testing.T) {
	na, nb, nc, fa, _, _, conn, tok := candidateSetup(t)
	idA := nc.e.ident.DeviceID()
	intro := func(c, k string, cs ...rpc.Candidate) error {
		return fa.auth.Introduce(bg, rpc.Introduce{ConnectionID: c, SessionToken: k, Candidates: cs})
	}
	if err := intro(conn, "0000000000000000000000000000000000000000000000000000000000000000"); errCode(err) != "permission_denied" {
		t.Fatalf("bad token: %q", errCode(err))
	}
	if err := intro("nope", tok); errCode(err) != "not_found" {
		t.Fatalf("unknown connection: %q", errCode(err))
	}
	err := intro(conn, tok,
		rpc.Candidate{ID: idA, Name: "A's laptop", Address: "fake://a"},
		rpc.Candidate{ID: na.e.ident.DeviceID(), Name: "me", Address: "x"},    // myself
		rpc.Candidate{ID: nb.e.ident.DeviceID(), Name: "intro", Address: "x"}, // the introducer
		rpc.Candidate{ID: "not-an-id", Name: "bad", Address: "x"})
	if err != nil {
		t.Fatal(err)
	}
	cs, err := na.e.ListCandidates(bg)
	if err != nil || len(cs) != 1 || cs[0].DeviceID != idA || cs[0].BackendID != "test.peer" || len(cs[0].Introducers) != 1 || cs[0].Introducers[0].DeviceID != nb.e.ident.DeviceID() {
		t.Fatalf("candidates: %+v %v", cs, err)
	}
	// Suggestion only: nothing about A is stored as a device.
	if _, err := na.e.st.Device(idA); err == nil {
		t.Fatal("a candidate became a device")
	}
	// The switch turns reception off.
	cur := state(t, na.e).Settings
	cur.Value.Sync.Introductions = false
	if _, err := na.e.UpdateSettings(bg, UpdateSettingsRequest{ExpectedRevision: cur.Revision, Value: cur.Value}); err != nil {
		t.Fatal(err)
	}
	other := "abcdefghijklmnopqrstuvwxyz"[:26]
	_ = intro(conn, tok, rpc.Candidate{ID: other, Name: "x", Address: "y"})
	if cs, _ = na.e.ListCandidates(bg); len(cs) != 1 {
		t.Fatalf("introduction accepted while off: %+v", cs)
	}
}

func TestPairCandidateRetriesUntilTheDeviceIsThere(t *testing.T) {
	na, nb, nc, fa, _, _, conn, tok := candidateSetup(t)
	idA := nc.e.ident.DeviceID()
	if err := fa.auth.Introduce(bg, rpc.Introduce{ConnectionID: conn, SessionToken: tok, Candidates: []rpc.Candidate{{ID: idA, Name: "A", Address: "fake://a"}}}); err != nil {
		t.Fatal(err)
	}
	// A is away.
	fa.pmu.Lock()
	fa.connErr = rpc.NewError(rpc.CodeUnavailable)
	fa.pmu.Unlock()
	op, err := na.e.PairCandidate(bg, PairCandidateRequest{RequestID: "p1", DeviceID: idA, BackendID: "test.peer"})
	if err != nil {
		t.Fatal(err)
	}
	waitOp(t, na.e, op, PhaseFailed)
	cs, _ := na.e.ListCandidates(bg)
	if len(cs) != 1 || !cs[0].Requested || cs[0].RequestedUntilMs == 0 {
		t.Fatalf("request not kept: %+v", cs)
	}
	// A comes back; the next retry pairs without anybody clicking again.
	fa.pmu.Lock()
	fa.connErr = nil
	fa.pmu.Unlock()
	na.e.retryCandidates()
	eventually(t, "retry pairing", func() bool { _, err := na.e.st.Device(idA); return err == nil })
	// A saw C as a new, unapproved device; C trusts nothing yet either.
	eventually(t, "far side recorded", func() bool { _, err := nc.e.st.Device(na.e.ident.DeviceID()); return err == nil })
	d, err := nc.e.st.Device(na.e.ident.DeviceID())
	if err != nil || d.Trust == "trusted" {
		t.Fatalf("far side: %+v %v", d, err)
	}
	eventually(t, "request cleared", func() bool {
		cs, _ := na.e.ListCandidates(bg)
		return len(cs) == 0
	})
	_ = nb
}

func TestCandidateIgnoreAndIntroducerRemoval(t *testing.T) {
	na, nb, nc, fa, _, _, conn, tok := candidateSetup(t)
	idA := nc.e.ident.DeviceID()
	if err := fa.auth.Introduce(bg, rpc.Introduce{ConnectionID: conn, SessionToken: tok, Candidates: []rpc.Candidate{{ID: idA, Name: "A", Address: "fake://a"}}}); err != nil {
		t.Fatal(err)
	}
	if err := na.e.IgnoreCandidate(bg, idA, true); err != nil {
		t.Fatal(err)
	}
	if cs, _ := na.e.ListCandidates(bg); len(cs) != 1 || !cs[0].Ignored {
		t.Fatalf("ignored: %+v", cs)
	}
	if err := na.e.IgnoreCandidate(bg, idA, false); err != nil {
		t.Fatal(err)
	}
	// Removing the introducer drops what only it vouched for, but nothing it
	// already led to.
	d, _ := na.e.st.Device(nb.e.ident.DeviceID())
	if err := na.e.UnpairDevice(bg, UnpairDeviceRequest{ID: d.ID, ExpectedRevision: d.Revision}); err != nil {
		t.Fatal(err)
	}
	if cs, _ := na.e.ListCandidates(bg); len(cs) != 0 {
		t.Fatalf("candidate outlived its introducer: %+v", cs)
	}
	if _, err := na.e.PairCandidate(bg, PairCandidateRequest{RequestID: "p", DeviceID: idA, BackendID: "test.peer"}); errCode2(err) != CodeNotFound {
		t.Fatalf("pairing a dropped candidate: %v", err)
	}
}

func errCode2(err error) Code {
	if e, ok := err.(*Error); ok {
		return e.Code
	}
	return ""
}

func TestProcessEndedDropsSessions(t *testing.T) {
	na, nb, _, fa, _, _, conn, tok := candidateSetup(t)
	idB := nb.e.ident.DeviceID()
	pa := na.e.peerProcs[0]
	if !pa.connected(idB) {
		t.Fatal("setup: not connected")
	}
	pa.ProcessEnded()
	if pa.connected(idB) {
		t.Fatal("session survived the process")
	}
	// The old token can never connect again; the new process must pass check_peer.
	if err := fa.auth.PeerEvent(bg, rpc.PeerEvent{ConnectionID: conn, SessionToken: tok, Sequence: 3, State: "connected", RemoteAccepted: true}); errCode(err) != "invalid_state" {
		t.Fatalf("revived: %q", errCode(err))
	}
	if pa.connected(idB) {
		t.Fatal("revived")
	}
}

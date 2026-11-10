package engine

import (
	"context"
	"crypto/sha256"
	"testing"

	"github.com/uniclipboard/unic/internal/clip"
)

// A backend hands Engine a clip it read from an authenticated sender. These
// tests drive that boundary directly, the way two backends delivering the
// same transfer would.

const sender = DeviceID("peer-1")

func dedupNode(t *testing.T, mutate func(*Options)) node {
	n := newContentNode(t, mutate)
	allowPeer(n.e, string(sender))
	start(t, n)
	return n
}

func clipReq(backend PluginID, transfer, view, text string) receiveRequest {
	return receiveRequest{backend: backend, source: sender, origin: string(sender), transfer: transfer, view: view,
		total: int64(len(text)), parts: []receivePart{{mime: MimeText, size: int64(len(text))}}}
}

// receiveVia runs one whole receive and reports whether it was admitted.
func receiveVia(e *Engine, req receiveRequest, text string) bool {
	adm, ok := e.admitReceive(req)
	if !ok {
		return false
	}
	adm.Done(clip.Snapshot{Parts: []clip.Part{{Format: MimeText, Data: []byte(text)}}}, "", true)
	return true
}

func receivedCount(t *testing.T, e *Engine) int {
	t.Helper()
	rs, err := e.ListReceived(bg)
	if err != nil {
		t.Fatal(err)
	}
	return len(rs)
}

func historyCount(t *testing.T, e *Engine) int {
	t.Helper()
	p, err := e.ListHistory(bg, ListHistoryRequest{Limit: 200})
	if err != nil {
		t.Fatal(err)
	}
	return len(p.Items)
}

func TestSameTransferViaTwoBackendsIsCommittedOnce(t *testing.T) {
	n := dedupNode(t, nil)
	if !receiveVia(n.e, clipReq("test.a", "t1", "h1", "hello"), "hello") {
		t.Fatal("the first arrival must be accepted")
	}
	if receiveVia(n.e, clipReq("test.b", "t1", "h1", "hello"), "hello") {
		t.Fatal("the same view through another backend must be dropped")
	}
	if receivedCount(t, n.e) != 1 || historyCount(t, n.e) != 1 {
		t.Fatalf("received=%d history=%d", receivedCount(t, n.e), historyCount(t, n.e))
	}
	// A different view of the same transfer, or another transfer, is independent.
	if !receiveVia(n.e, clipReq("test.b", "t1", "h2", "hello!"), "hello!") || !receiveVia(n.e, clipReq("test.b", "t2", "h1", "hello"), "hello") {
		t.Fatal("different views must not be merged")
	}
	if receivedCount(t, n.e) != 3 || historyCount(t, n.e) != 3 {
		t.Fatalf("received=%d history=%d", receivedCount(t, n.e), historyCount(t, n.e))
	}
	// The identity is the authenticated sender: another device's identical IDs do not collide.
	other := clipReq("test.a", "t1", "h1", "hello")
	other.source, other.origin = "peer-2", "peer-2"
	allowPeer(n.e, "peer-2")
	if !receiveVia(n.e, other, "hello") {
		t.Fatal("a different sender must not be deduplicated against this one")
	}
}

func TestConcurrentArrivalsAndFailedFirstCopy(t *testing.T) {
	n := dedupNode(t, nil)
	first, ok := n.e.admitReceive(clipReq("test.a", "t1", "h1", "hello"))
	if !ok {
		t.Fatal("first copy")
	}
	if _, ok := n.e.admitReceive(clipReq("test.b", "t1", "h1", "hello")); ok {
		t.Fatal("the second copy must wait out, not race, the first")
	}
	// The first copy fails; its placeholder is gone and a new arrival may try.
	first.Done(clip.Snapshot{}, "", false)
	if !receiveVia(n.e, clipReq("test.b", "t1", "h1", "hello"), "hello") {
		t.Fatal("a failed copy must not block the transfer for good")
	}
	if receivedCount(t, n.e) != 1 {
		t.Fatal("exactly one copy was committed")
	}
	if stored(n.e) != 5 {
		t.Fatalf("storage = %d, reservations leaked", stored(n.e))
	}
}

func TestPersistFalseStillRemembersTheTransfer(t *testing.T) {
	n := newContentNode(t, nil)
	allowPeer(n.e, string(sender))
	n.e.processors = []*pipelineProcessor{newPipelineProcessor("nop", []pipelineEvent{pipelineReceived}, false, func(_ context.Context, c *processingContext) error {
		return c.SetPersist(false)
	})}
	start(t, n)
	if !receiveVia(n.e, clipReq("test.a", "t1", "h1", "secret"), "secret") {
		t.Fatal("accepted")
	}
	if historyCount(t, n.e) != 0 || receivedCount(t, n.e) != 1 {
		t.Fatal("persist=false must keep the clip out of history but still receive it")
	}
	if receiveVia(n.e, clipReq("test.b", "t1", "h1", "secret"), "secret") {
		t.Fatal("persist=false must still record the transfer as done")
	}
}

func TestDedupSurvivesRestartAndIsBounded(t *testing.T) {
	dir := t.TempDir()
	open := func() *Engine {
		opts, _ := newOpts(t, dir)
		opts.Resources.MaxSyncDedupEntries = 2
		e, err := New(bg, opts)
		if err != nil {
			t.Fatal(err)
		}
		allowPeer(e, string(sender))
		if err := e.Start(bg); err != nil {
			t.Fatal(err)
		}
		return e
	}
	e := open()
	if !receiveVia(e, clipReq("test.a", "t1", "h", "one"), "one") {
		t.Fatal("t1")
	}
	_ = e.Close(bg)
	e = open()
	defer func() { _ = e.Close(bg) }()
	if receiveVia(e, clipReq("test.b", "t1", "h", "one"), "one") {
		t.Fatal("the record must survive a restart")
	}
	// Capacity two: after t2 and t3, the oldest (t1) is forgotten.
	for _, id := range []string{"t2", "t3"} {
		if !receiveVia(e, clipReq("test.a", id, "h", "x"), "x") {
			t.Fatal(id)
		}
	}
	if !receiveVia(e, clipReq("test.b", "t1", "h", "one"), "one") {
		t.Fatal("an evicted record no longer protects against a repeat")
	}
}

func TestAdmissionRefusals(t *testing.T) {
	n := dedupNode(t, nil)
	cases := map[string]func(*receiveRequest){
		"origin differs from the authenticated sender": func(r *receiveRequest) { r.origin = "someone-else" },
		"empty transfer id":                            func(r *receiveRequest) { r.transfer = "" },
		"empty view hash":                              func(r *receiveRequest) { r.view = "" },
		"nothing to receive":                           func(r *receiveRequest) { r.total = 0 },
	}
	for name, mut := range cases {
		req := clipReq("test.a", "t-"+name, "h", "abc")
		mut(&req)
		if receiveVia(n.e, req, "abc") {
			t.Fatalf("%s: was admitted", name)
		}
	}
	if stored(n.e) != 0 {
		t.Fatalf("a refused clip reserved %d bytes", stored(n.e))
	}
	// This backend does not take text: refused before a byte is read.
	s := state(t, n.e).Settings
	s.Value.BackendPolicies["test.a"] = BackendSyncPolicy{Images: true}
	if _, err := n.e.UpdateSettings(bg, UpdateSettingsRequest{ExpectedRevision: s.Revision, Value: s.Value}); err == nil {
		t.Fatal("test.a is not an assembled backend, its policy must be rejected")
	}
	n2 := dedupNode(t, func(o *Options) { o.extraBackends = []syncBackend{&fakeBackend{id: "test.a"}} })
	s = state(t, n2.e).Settings
	s.Value.BackendPolicies["test.a"] = BackendSyncPolicy{Images: true}
	if _, err := n2.e.UpdateSettings(bg, UpdateSettingsRequest{ExpectedRevision: s.Revision, Value: s.Value}); err != nil {
		t.Fatal(err)
	}
	if receiveVia(n2.e, clipReq("test.a", "t1", "h", "text"), "text") {
		t.Fatal("a backend whose policy filters every part must not be asked for the bytes")
	}
	if !receiveVia(n2.e, clipReq(BackendTailcat, "t1", "h", "text"), "text") {
		t.Fatal("another backend with a permissive policy still receives it")
	}
	// Sync off refuses everything.
	updateSettings(t, n2.e, func(s *Settings) { s.Sync.Enabled = false })
	if receiveVia(n2.e, clipReq(BackendTailcat, "t9", "h", "text"), "text") {
		t.Fatal("received while sync was off")
	}
}

// allowPeer makes id a trusted device with an authorized tailcat path, the way
// a finished pairing would, without a network.
func allowPeer(e *Engine, id string) {
	pk := sha256.Sum256([]byte(id))
	d, err := e.st.AddDevice(id, pk[:], "peer")
	if err != nil {
		panic(err)
	}
	if _, err := e.st.TrustDevice(id, d.Revision); err != nil {
		panic(err)
	}
	// receiving checks the binding of the backend it came through
	for _, inst := range []string{tailcatInstance, "test.a", "test.b"} {
		b, err := e.st.AddBinding(randHex(8), id, inst, []byte("ctx"))
		if err != nil {
			panic(err)
		}
		if _, err := e.st.AuthorizeBinding(b.ID, b.Revision); err != nil {
			panic(err)
		}
	}
}

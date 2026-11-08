package engine

import (
	"errors"
	"testing"

	"github.com/uniclipboard/unic/plugin"
)

func channelNode(t *testing.T, b *storeBackend) node {
	b.chans = true
	if b.kind == "" {
		b.kind = plugin.TransportObjectStore
	}
	n := pluginBackendNode(t, b)
	start(t, n)
	return n
}

func addCh(t *testing.T, n node, id string) Channel {
	c, err := n.e.AddChannel(bg, AddChannelRequest{BackendID: PluginID(id), RemoteContext: []byte("group-secret-" + id), ReportedName: "WeType"})
	if err != nil {
		t.Fatal(err)
	}
	return c
}

func TestChannelLifecycleAndMixedList(t *testing.T) {
	n := channelNode(t, &storeBackend{id: "test.wx"})
	c := addCh(t, n, "test.wx")
	if !c.Enabled || !c.AutoApply || c.TargetKind != TargetGroup || c.RemoteRevocation {
		t.Fatalf("defaults: %+v", c)
	}
	if again := addCh(t, n, "test.wx"); again.ID != c.ID {
		t.Fatal("same remote context must return the same channel")
	}
	if _, err := n.e.AddChannel(bg, AddChannelRequest{BackendID: "nope", RemoteContext: []byte("x"), ReportedName: "n"}); !errors.Is(err, &Error{Code: CodeNotFound}) && err == nil {
		t.Fatal("unknown backend must fail")
	}
	r, err := n.e.RenameChannel(bg, RenameChannelRequest{ID: c.ID, ExpectedRevision: c.Revision, Name: "Phone group"})
	if err != nil || r.DisplayName() != "Phone group" {
		t.Fatalf("%+v %v", r, err)
	}
	if _, err := n.e.RenameChannel(bg, RenameChannelRequest{ID: c.ID, ExpectedRevision: c.Revision, Name: "stale"}); err == nil {
		t.Fatal("stale revision must conflict")
	}
	ps, err := n.e.ListPeers(bg)
	if err != nil {
		t.Fatal(err)
	}
	var chans int
	for _, p := range ps {
		if p.Kind == PeerChannel && p.Channel != nil && p.Device == nil {
			chans++
		}
	}
	if chans != 1 {
		t.Fatalf("peers = %+v", ps)
	}
}

func TestChannelGatesSending(t *testing.T) {
	b := &storeBackend{id: "test.wx"}
	n := channelNode(t, b)
	send := func(id string) SubmissionResult {
		h := importText(t, n.e, id)
		r, err := n.e.CommitContent(bg, CommitRequest{RequestID: RequestID(id), LeaseID: h.LeaseID})
		if err != nil {
			t.Fatal(err)
		}
		return r
	}
	if r := send("none"); r.SyncDisposition == SyncQueued {
		t.Fatalf("no channel: nothing to send: %+v", r)
	}
	c := addCh(t, n, "test.wx")
	r := send("one")
	if r.SyncDisposition != SyncQueued {
		t.Fatalf("%+v", r)
	}
	if op, _ := n.e.GetOperation(bg, r.OperationID); len(op.Targets) != 1 || op.Targets[0].TargetKind != TargetGroup || op.Targets[0].TargetID != c.ID {
		t.Fatalf("one group target expected: %+v", op.Targets)
	}
	d, err := n.e.SetChannelEnabled(bg, SetChannelEnabledRequest{ID: c.ID, ExpectedRevision: c.Revision, Enabled: false})
	if err != nil {
		t.Fatal(err)
	}
	if r := send("off"); r.SyncDisposition == SyncQueued {
		t.Fatalf("disabled channel must not be offered: %+v", r)
	}
	b.mu.Lock()
	h := b.host
	b.mu.Unlock()
	if err := h.Register([]string{d.ID}); err == nil {
		t.Fatal("registering a disabled channel must fail")
	}
}

func TestChannelRemoveLocalOnly(t *testing.T) {
	b := &storeBackend{id: "test.wx"}
	n := channelNode(t, b)
	c := addCh(t, n, "test.wx")
	if err := n.e.RemoveChannel(bg, RemoveChannelRequest{ID: c.ID, ExpectedRevision: c.Revision}); err != nil {
		t.Fatal(err)
	}
	if len(b.left) != 0 {
		t.Fatal("no remote leave without remote_revocation")
	}
	if _, err := n.e.GetChannel(bg, c.ID); err == nil {
		t.Fatal("channel must be gone")
	}
}

func TestChannelRemoveWithRemoteRevocation(t *testing.T) {
	b := &storeBackend{id: "test.wx", revoke: true, leave: errors.New("net down")}
	n := channelNode(t, b)
	c := addCh(t, n, "test.wx")
	if !c.RemoteRevocation {
		t.Fatal("flag must surface")
	}
	err := n.e.RemoveChannel(bg, RemoveChannelRequest{ID: c.ID, ExpectedRevision: c.Revision})
	if err == nil {
		t.Fatal("leave failure must fail removal")
	}
	kept, gerr := n.e.GetChannel(bg, c.ID)
	if gerr != nil || kept.Enabled || kept.LastError != "remove_failed" {
		t.Fatalf("record must be kept, disabled, marked: %+v %v", kept, gerr)
	}
	b.mu.Lock()
	b.leave = nil
	b.mu.Unlock()
	if err := n.e.RemoveChannel(bg, RemoveChannelRequest{ID: kept.ID, ExpectedRevision: kept.Revision}); err != nil {
		t.Fatal(err)
	}
	if len(b.left) != 1 {
		t.Fatalf("left = %v", b.left)
	}
	if _, err := n.e.GetChannel(bg, c.ID); err == nil {
		t.Fatal("channel must be gone after a successful leave")
	}
}

func TestChannelSpecRules(t *testing.T) {
	cases := map[string]*storeBackend{
		"revoke without channels": {id: "test.a", kind: plugin.TransportObjectStore, revoke: true},
	}
	for name, b := range cases {
		if _, err := New(bg, Options{DataDir: t.TempDir(), Plugins: []PluginInstance{{Plugin: b}}}); err == nil {
			t.Errorf("%s: expected an error", name)
		}
	}
}

func TestChannelReceive(t *testing.T) {
	b := &storeBackend{id: "test.wx"}
	n := channelNode(t, b)
	c := addCh(t, n, "test.wx")
	msg := plugin.Received{Transfer: "t1", View: "v1", Parts: []plugin.ReceivedPart{{MIME: MimeText, Data: []byte("from phone")}}}
	if err := b.rcv.Receive(bg, c.ID, msg); err != nil {
		t.Fatal(err)
	}
	rs := received(n.e)
	if len(rs) != 1 || rs[0].ChannelID != c.ID || rs[0].PeerID != "" || !rs[0].AutoApply {
		t.Fatalf("received = %+v", rs)
	}
	page, err := n.e.ListHistory(bg, ListHistoryRequest{Filter: HistoryFilter{OriginKinds: []HistoryOriginKind{OriginKindExternal}}})
	if err != nil || len(page.Items) != 1 || page.Items[0].Origin.PluginID != "test.wx" || page.Items[0].Origin.DeviceID != "" {
		t.Fatalf("history = %+v %v", page, err)
	}
	if err := b.rcv.Receive(bg, c.ID, msg); err == nil {
		t.Fatal("the same transfer must be dropped")
	}
	c2, _ := n.e.SetChannelAutoApply(bg, SetChannelAutoApplyRequest{ID: c.ID, ExpectedRevision: c.Revision, AutoApply: false})
	msg.Transfer, msg.View = "t2", "v2"
	if err := b.rcv.Receive(bg, c.ID, msg); err != nil {
		t.Fatal(err)
	}
	if rs = received(n.e); len(rs) != 2 || rs[len(rs)-1].AutoApply && rs[0].AutoApply {
		t.Fatalf("auto-apply off must show on the new entry: %+v", rs)
	}
	d, _ := n.e.SetChannelEnabled(bg, SetChannelEnabledRequest{ID: c.ID, ExpectedRevision: c2.Revision, Enabled: false})
	msg.Transfer, msg.View = "t3", "v3"
	if err := b.rcv.Receive(bg, c.ID, msg); err == nil {
		t.Fatalf("a disabled channel delivers nothing (%+v)", d)
	}
	if err := b.rcv.Receive(bg, "unknown", msg); err == nil {
		t.Fatal("unknown channel")
	}
}

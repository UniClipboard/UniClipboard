package engine

import (
	"context"
	"errors"
	"fmt"
	"slices"
	"sync"
	"testing"
	"time"

	"github.com/uniclipboard/unic/plugin"
)

type fakeObserver struct {
	fakePlugin
	mu     sync.Mutex
	got    []plugin.Event
	block  chan struct{} // when set, OnEvent waits for it (ignoring ctx)
	failed bool
}

func (f *fakeObserver) OnEvent(_ context.Context, ev plugin.Event) error {
	if f.block != nil {
		<-f.block
	}
	f.mu.Lock()
	f.got = append(f.got, ev)
	f.mu.Unlock()
	if f.failed {
		panic("observer failure")
	}
	return nil
}

func (f *fakeObserver) kinds() []string {
	f.mu.Lock()
	defer f.mu.Unlock()
	var out []string
	for _, ev := range f.got {
		out = append(out, ev.Kind+" "+ev.ItemID)
	}
	return out
}

func newFakeObserver(id string) *fakeObserver {
	var closed []string
	return &fakeObserver{fakePlugin: fakePlugin{spec: plugin.Spec{ID: id, Version: "1", APIVersion: 1, Capabilities: []string{plugin.CapObserver}}, closed: &closed}}
}

func TestObserverSeesHistoryLifecycle(t *testing.T) {
	historySweepEvery = 20 * time.Millisecond
	t.Cleanup(func() { historySweepEvery = 30 * time.Second })
	obs := newFakeObserver("example.obs")
	n := newContentNode(t, func(o *Options) { o.Plugins = []PluginInstance{{Plugin: obs}} })
	start(t, n)
	a := commitText(t, n.e, "o1", "first")
	tags := []string{"x"}
	if _, err := n.e.UpdateItem(bg, UpdateItemRequest{ID: a.HistoryID, ExpectedRevision: 1, Tags: &tags}); err != nil {
		t.Fatal(err)
	}
	if err := n.e.DeleteItem(bg, DeleteItemRequest{ID: a.HistoryID, ExpectedRevision: 2}); err != nil {
		t.Fatal(err)
	}
	b := commitText(t, n.e, "o2", "second")
	at := time.Now().Add(100 * time.Millisecond).UnixMilli()
	if _, err := n.e.UpdateItem(bg, UpdateItemRequest{ID: b.HistoryID, ExpectedRevision: 1, ExpiresAtMs: &at}); err != nil {
		t.Fatal(err)
	}
	id1, id2 := string(a.HistoryID), string(b.HistoryID)
	want := []string{"app.started ", "clipboard.committed " + id1, "clipboard.updated " + id1, "clipboard.deleted " + id1,
		"clipboard.committed " + id2, "clipboard.updated " + id2, "clipboard.expired " + id2}
	eventually(t, "all events delivered in order", func() bool { return slices.Equal(obs.kinds(), want) })
}

func TestObserverSubscriptionFilters(t *testing.T) {
	obs := newFakeObserver("example.obs")
	n := newContentNode(t, func(o *Options) {
		o.Plugins = []PluginInstance{{Plugin: obs, Events: []string{plugin.EventDeleted}}}
	})
	start(t, n)
	r := commitText(t, n.e, "f1", "something")
	if err := n.e.DeleteItem(bg, DeleteItemRequest{ID: r.HistoryID, ExpectedRevision: 1}); err != nil {
		t.Fatal(err)
	}
	eventually(t, "only the subscribed event", func() bool { return slices.Equal(obs.kinds(), []string{"clipboard.deleted " + string(r.HistoryID)}) })
}

func TestSlowOrBrokenObserverNeverBlocksTheEngine(t *testing.T) {
	obs := newFakeObserver("example.obs")
	obs.block = make(chan struct{})
	broken := newFakeObserver("example.broken")
	broken.failed = true
	n := newContentNode(t, func(o *Options) { o.Plugins = []PluginInstance{{Plugin: obs}, {Plugin: broken}} })
	start(t, n)
	begin := time.Now()
	for i := 0; i < 3*observerQueue; i++ {
		tag := fmt.Sprintf("%03d", i)
		commitText(t, n.e, "s"+tag, "item "+tag)
	}
	if d := time.Since(begin); d > 20*time.Second {
		t.Fatalf("commits stalled for %v", d)
	}
	eventually(t, "a panicking observer keeps receiving", func() bool { return len(broken.kinds()) >= 3*observerQueue/2 })
	close(obs.block)
	if err := n.e.Close(bg); err != nil { // must not hang on the stuck observer
		t.Fatal(err)
	}
}

func TestObserverAndTransportValidation(t *testing.T) {
	var closed []string
	spec := func(caps ...string) plugin.Spec {
		return plugin.Spec{ID: "x.y", Version: "1", APIVersion: 1, Capabilities: caps}
	}
	obsWith := func(s plugin.Spec) plugin.Plugin {
		return &fakeObserver{fakePlugin: fakePlugin{spec: s, closed: &closed}}
	}
	withTransport := spec(plugin.CapObserver)
	withTransport.TransportKind = "peer_sessions"
	cases := map[string]PluginInstance{
		"processor event on observer": {Plugin: obsWith(spec(plugin.CapObserver)), Events: []string{plugin.EventCaptured}},
		"observer event on processor": {Plugin: fakeProcessor{fakePlugin{spec: spec(plugin.CapProcessor), closed: &closed}}, Events: []string{plugin.EventDeleted}},
		"repeated event":              {Plugin: obsWith(spec(plugin.CapObserver)), Events: []string{plugin.EventDeleted, plugin.EventDeleted}},
		"transport kind":              {Plugin: obsWith(withTransport)},
		"observer not implemented":    {Plugin: fakeProcessor{fakePlugin{spec: spec(plugin.CapObserver), closed: &closed}}},
		"observer undeclared":         {Plugin: obsWith(spec())},
	}
	for name, in := range cases {
		opts, _ := newOpts(t, t.TempDir())
		opts.Plugins = []PluginInstance{in}
		if e, err := New(bg, opts); err == nil {
			_ = e.Close(bg)
			t.Errorf("%s: accepted", name)
		} else if code(err) != CodeInvalidArgument {
			t.Errorf("%s: %v", name, err)
		}
	}
}

func TestDisabledPluginsAreCheckedButNotStarted(t *testing.T) {
	var closed []string
	p := fakeProcessor{fakePlugin{spec: plugin.Spec{ID: "off.one", Version: "1", APIVersion: 1, Capabilities: []string{plugin.CapProcessor}}, initErr: errors.New("must not run"), closed: &closed}}
	opts, _ := newOpts(t, t.TempDir())
	opts.Plugins = []PluginInstance{{Plugin: p, Disabled: true}}
	e, err := New(bg, opts)
	if err != nil {
		t.Fatalf("a disabled plugin was initialized: %v", err)
	}
	_ = e.Close(bg)
	if len(closed) != 0 {
		t.Fatalf("closed %v", closed)
	}
	// its ID stays reserved and its description must still be valid
	opts.Plugins = []PluginInstance{{Plugin: p, Disabled: true}, {Plugin: p}}
	if _, err := New(bg, opts); code(err) != CodeInvalidArgument {
		t.Fatalf("duplicate with a disabled entry: %v", err)
	}
	bad := p
	bad.spec.APIVersion = 7
	opts.Plugins = []PluginInstance{{Plugin: bad, Disabled: true}}
	if _, err := New(bg, opts); code(err) != CodeInvalidArgument {
		t.Fatalf("invalid disabled description: %v", err)
	}
}

func TestSyncBackendDescriptions(t *testing.T) {
	backend := func(id, kind string) PluginInstance {
		var closed []string
		return PluginInstance{Plugin: fakePlugin{spec: plugin.Spec{ID: id, Version: "1", APIVersion: 1,
			Capabilities: []string{plugin.CapSyncBackend}, TransportKind: kind}, closed: &closed}}
	}
	try := func(max int, list ...PluginInstance) Code {
		opts, _ := newOpts(t, t.TempDir())
		opts.Resources.MaxSyncBackends = max
		opts.Plugins = list
		e, err := New(bg, opts)
		if err == nil {
			_ = e.Close(bg)
			return ""
		}
		return code(err)
	}
	if c := try(0, backend("net.one", "")); c != CodeInvalidArgument {
		t.Fatalf("no transport kind: %v", c)
	}
	if c := try(0, backend("net.one", "carrier_pigeon")); c != CodeInvalidArgument {
		t.Fatalf("unknown transport kind: %v", c)
	}
	if c := try(0, backend("net.one", plugin.TransportObjectStore)); c != CodeUnsupported {
		t.Fatalf("object store: %v", c)
	}
	off := backend("net.two", plugin.TransportPeerSessions)
	off.Disabled = true
	if c := try(0, off); c != "" {
		t.Fatalf("a disabled backend must not stop the engine: %v", c)
	}
	if c := try(1, backend("net.one", plugin.TransportPeerSessions), backend("net.two", plugin.TransportPeerSessions)); c != CodeLimitExceeded {
		t.Fatalf("more enabled backends than allowed: %v", c)
	}
	if c := try(2, backend("net.one", plugin.TransportPeerSessions), backend("net.two", plugin.TransportPeerSessions)); c != CodeUnsupported {
		t.Fatalf("backends within the limit: %v", c)
	}
	if c := try(33); c != CodeInvalidArgument {
		t.Fatalf("limit above 32: %v", c)
	}
	if c := try(-1); c != CodeInvalidArgument {
		t.Fatalf("negative limit: %v", c)
	}
}

func TestObserverSeesPastedAndLifecycle(t *testing.T) {
	obs := newFakeObserver("example.obs")
	n := newContentNode(t, func(o *Options) { withHost(o); o.Plugins = []PluginInstance{{Plugin: obs}} })
	start(t, n)
	eventually(t, "app.started", func() bool { return slices.Contains(obs.kinds(), "app.started ") })
	hb := newHost(t, n.e)
	hb.must(ObserveBaseline, "")
	r := commitText(t, n.e, "p1", "paste me")
	tk, err := n.e.BeginApplyHistory(bg, ApplyHistoryRequest{HistoryID: r.HistoryID, ExpectedEpoch: hb.epoch, ExpectedSequence: hb.seq})
	if err != nil {
		t.Fatal(err)
	}
	if res, err := hb.observe(ObserveChange, "paste me", tk.Token); err != nil || res.Disposition != DispEcho {
		t.Fatalf("echo = %+v, %v", res, err)
	}
	back := importText(t, n.e, "paste me")
	hb.seq++
	if err := n.e.ReportApplyResult(bg, ApplyResult{Token: tk.Token, Outcome: ApplyOutcomeApplied, BaselineLeaseID: back.LeaseID, Epoch: hb.epoch, Sequence: hb.seq}); err != nil {
		t.Fatal(err)
	}
	eventually(t, "pasted", func() bool { return slices.Contains(obs.kinds(), "clipboard.pasted "+string(r.HistoryID)) })
	if err := n.e.Close(bg); err != nil {
		t.Fatal(err)
	}
	if !slices.Contains(obs.kinds(), "app.shutdown ") {
		t.Fatalf("shutdown was not delivered: %v", obs.kinds())
	}
}

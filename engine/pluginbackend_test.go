package engine

import (
	"context"
	"errors"
	"io"
	"strings"
	"sync"
	"testing"

	"github.com/uniclipboard/unic/plugin"
)

// storeBackend is a sync_backend plugin that records what it is offered.
type storeBackend struct {
	id      string
	kind    string
	targets []string
	err     error
	failOn  string // Init or Start
	chans   bool   // declares external channels
	revoke  bool   // declares remote revocation
	leave   error
	left    []string
	rcv     plugin.Receiver

	mu     sync.Mutex
	views  []plugin.SyncView
	host   plugin.SyncHost
	closed bool
}

func (s *storeBackend) Spec() plugin.Spec {
	return plugin.Spec{ID: s.id, Version: "1", APIVersion: 1, Capabilities: []string{plugin.CapSyncBackend}, TransportKind: s.kind, ExternalChannels: s.chans, RemoteRevocation: s.revoke}
}
func (s *storeBackend) SetReceiver(r plugin.Receiver) { s.rcv = r }
func (s *storeBackend) LeaveChannel(_ context.Context, id string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.leave != nil {
		return s.leave
	}
	s.left = append(s.left, id)
	return nil
}
func (s *storeBackend) Init(context.Context, plugin.Host, plugin.Config) error {
	if s.failOn == "init" {
		return errors.New("boom")
	}
	return nil
}
func (s *storeBackend) Start(context.Context) error {
	if s.failOn == "start" {
		return errors.New("boom")
	}
	return nil
}
func (s *storeBackend) Close(context.Context) error {
	s.mu.Lock()
	s.closed = true
	s.mu.Unlock()
	return nil
}
func (s *storeBackend) Submit(_ context.Context, v plugin.SyncView, h plugin.SyncHost) (bool, error) {
	if s.err != nil {
		return false, s.err
	}
	targets := s.targets
	if s.chans {
		targets = v.Targets
	}
	if len(targets) == 0 {
		return false, nil
	}
	if err := h.Register(targets); err != nil {
		return false, err
	}
	s.mu.Lock()
	s.views, s.host = append(s.views, v), h
	s.mu.Unlock()
	return true, nil
}

func pluginBackendNode(t *testing.T, bs ...*storeBackend) node {
	return newContentNode(t, func(o *Options) {
		for _, b := range bs {
			o.Plugins = append(o.Plugins, PluginInstance{Plugin: b})
		}
	})
}

func TestPluginBackendsRunBesideEachOther(t *testing.T) {
	a := &storeBackend{id: "test.repo", kind: plugin.TransportObjectStore, targets: []string{"r1"}}
	b := &storeBackend{id: "test.peer", kind: plugin.TransportPeerSessions, err: errors.New("secret-token-123")}
	n := pluginBackendNode(t, a, b)
	start(t, n)
	h := importText(t, n.e, "hello")
	r, err := n.e.CommitContent(bg, CommitRequest{RequestID: "pb", LeaseID: h.LeaseID})
	if err != nil || r.SyncDisposition != SyncQueued {
		t.Fatalf("%+v %v", r, err)
	}
	if resultOf(r, "test.repo").Disposition != BackendQueued {
		t.Fatalf("%+v", r.BackendResults)
	}
	fb := resultOf(r, "test.peer")
	if fb.Disposition != BackendFailed || fb.Error == nil || fb.Error.Message == "" || strings.Contains(fb.Error.Message, "secret") {
		t.Fatalf("failure must be isolated and carry no plugin text: %+v", fb)
	}
	a.mu.Lock()
	v := a.views[0]
	a.mu.Unlock()
	if len(v.Parts) != 1 || v.Parts[0].MIME != MimeText || v.ID == "" {
		t.Fatalf("view = %+v", v)
	}
	rc, err := v.Parts[0].Open(bg)
	if err != nil {
		t.Fatal(err)
	}
	if b, _ := io.ReadAll(rc); string(b) != "hello" {
		t.Fatalf("read %q", b)
	}
	_ = rc.Close()
	op, _ := n.e.GetOperation(bg, r.OperationID)
	if len(op.Targets) != 1 || op.Targets[0].TargetKind != TargetRepository || op.Targets[0].BackendID != "test.repo" {
		t.Fatalf("targets = %+v", op.Targets)
	}
	a.host.Report("r1", plugin.SyncSent, nil)
	if op, _ = n.e.GetOperation(bg, r.OperationID); op.Phase != PhaseCompleted {
		t.Fatalf("op = %+v", op)
	}
}

func TestPluginBackendLoadFailureClosesStartedOnes(t *testing.T) {
	a := &storeBackend{id: "test.one", kind: plugin.TransportObjectStore}
	bad := &storeBackend{id: "test.two", kind: plugin.TransportObjectStore, failOn: "start"}
	_, err := New(bg, Options{DataDir: t.TempDir(), Plugins: []PluginInstance{{Plugin: a}, {Plugin: bad}}})
	if err == nil {
		t.Fatal("expected a failure")
	}
	a.mu.Lock()
	defer a.mu.Unlock()
	if !a.closed {
		t.Fatal("the backend started before the failure must be closed")
	}
}

func TestPluginBackendRejections(t *testing.T) {
	cases := map[string][]PluginInstance{
		"duplicate": {{Plugin: &storeBackend{id: "test.x", kind: plugin.TransportObjectStore}}, {Plugin: &storeBackend{id: "test.x", kind: plugin.TransportObjectStore}}},
		"reserved":  {{Plugin: &storeBackend{id: string(BackendTailcat), kind: plugin.TransportPeerSessions}}},
		"no kind":   {{Plugin: &storeBackend{id: "test.y"}}},
	}
	for name, list := range cases {
		if _, err := New(bg, Options{DataDir: t.TempDir(), Plugins: list}); err == nil {
			t.Errorf("%s: expected an error", name)
		}
	}
}

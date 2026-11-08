package host

import (
	"context"
	"testing"

	"github.com/uniclipboard/unic/app/desktop/internal/api"
	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/plugin"
)

// chanBackend is a minimal external-channel backend that only receives.
type chanBackend struct{ rcv plugin.Receiver }

func (c *chanBackend) Spec() plugin.Spec {
	return plugin.Spec{ID: "test.wx", Version: "1", APIVersion: 1, Capabilities: []string{plugin.CapSyncBackend},
		TransportKind: plugin.TransportObjectStore, ExternalChannels: true}
}
func (c *chanBackend) Init(context.Context, plugin.Host, plugin.Config) error { return nil }
func (c *chanBackend) Start(context.Context) error                            { return nil }
func (c *chanBackend) Close(context.Context) error                            { return nil }
func (c *chanBackend) SetReceiver(r plugin.Receiver)                          { c.rcv = r }
func (c *chanBackend) Submit(context.Context, plugin.SyncView, plugin.SyncHost) (bool, error) {
	return false, nil
}

func TestExternalChannelInPeerListAndAutoApply(t *testing.T) {
	cb := &chanBackend{}
	h := newHostWith(t, false, []engine.PluginInstance{{Plugin: cb}})
	ch, err := h.n.e.AddChannel(bg, engine.AddChannelRequest{BackendID: "test.wx", RemoteContext: []byte("grp"), ReportedName: "WeType"})
	if err != nil {
		t.Fatal(err)
	}
	st, _ := h.n.Status(bg)
	var found bool
	for _, p := range st.Peers {
		if p.ID == ch.ID {
			found = p.Kind == "external" && p.Enabled && p.AutoApply && !p.RemoteRevocation && p.Name == "WeType" && p.Revision != 0
		}
	}
	if !found {
		t.Fatalf("peers = %+v", st.Peers)
	}
	events, stop := h.n.Subscribe()
	defer stop()

	if err := cb.rcv.Receive(bg, ch.ID, plugin.Received{Transfer: "1", View: "a", Parts: []plugin.ReceivedPart{{MIME: "text/plain", Data: []byte("hi")}}}); err != nil {
		t.Fatal(err)
	}
	eventually(t, "auto applied", func() bool { return got(h) == "hi" })

	// Automatic writing off: the item arrives, the clipboard stays.
	p, err := h.n.UpdateChannel(bg, ch.ID, apiUpdate(st, ch.ID, false))
	if err != nil || p.AutoApply {
		t.Fatalf("%+v %v", p, err)
	}
	if err := cb.rcv.Receive(bg, ch.ID, plugin.Received{Transfer: "2", View: "b", Parts: []plugin.ReceivedPart{{MIME: "text/plain", Data: []byte("quiet")}}}); err != nil {
		t.Fatal(err)
	}
	eventually(t, "listed", func() bool { r, _ := h.n.e.ListReceived(bg); return len(r) >= 1 })
	if got(h) != "hi" {
		t.Fatalf("clipboard = %q", got(h))
	}
	sawChannel := false
	for len(events) > 0 {
		if ev := <-events; ev.Kind == "channel" && ev.Info == ch.ID {
			sawChannel = true
		}
	}
	if !sawChannel {
		t.Fatal("expected a channel event")
	}

	if err := h.n.RemoveChannel(bg, ch.ID, p.Revision); err != nil {
		t.Fatal(err)
	}
	st, _ = h.n.Status(bg)
	for _, p := range st.Peers {
		if p.ID == ch.ID {
			t.Fatal("removed channel still listed")
		}
	}
}

func apiUpdate(st api.Status, id string, auto bool) api.ChannelUpdate {
	for _, p := range st.Peers {
		if p.ID == id {
			return api.ChannelUpdate{Revision: p.Revision, AutoApply: &auto}
		}
	}
	panic("no such peer")
}

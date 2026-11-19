//go:build unic_e2e

package main

import (
	"context"
	"encoding/json"
	"net/http"
	"os"
	"sync"
	"sync/atomic"

	"github.com/uniclipboard/unic/app/service/internal/host"
	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/plugin"
)

// Opt-in fixture plugins use public plugin and Engine interfaces. They produce
// business work, never diagnostic records or synthesized telemetry results.
var followupChannel *followupExternalBackend
var followupObserver *followupPanicObserver

func configureSyncFollowup(o *host.Options) {
	if os.Getenv("UNIC_E2E_SYNC_FOLLOWUP") != "1" {
		return
	}
	followupChannel = &followupExternalBackend{}
	followupObserver = &followupPanicObserver{}
	o.Plugins = append(o.Plugins, engine.PluginInstance{Plugin: followupChannel},
		engine.PluginInstance{Plugin: followupObserver, Events: []string{plugin.EventCommitted}})
}

type followupExternalBackend struct {
	receiver plugin.Receiver
	mu       sync.Mutex
	channel  string
}

func (*followupExternalBackend) Spec() plugin.Spec {
	return plugin.Spec{ID: "test.externalfollowup", Version: "1", APIVersion: 1, Capabilities: []string{plugin.CapSyncBackend},
		TransportKind: plugin.TransportObjectStore, ExternalChannels: true}
}
func (*followupExternalBackend) Init(context.Context, plugin.Host, plugin.Config) error { return nil }
func (*followupExternalBackend) Start(context.Context) error                            { return nil }
func (*followupExternalBackend) Close(context.Context) error                            { return nil }
func (b *followupExternalBackend) SetReceiver(receiver plugin.Receiver)                 { b.receiver = receiver }
func (*followupExternalBackend) Submit(context.Context, plugin.SyncView, plugin.SyncHost) (bool, error) {
	return false, nil
}

type followupPanicObserver struct{ committed atomic.Uint64 }

func (*followupPanicObserver) Spec() plugin.Spec {
	return plugin.Spec{ID: "test.panicobserver", Version: "1", APIVersion: 1, Capabilities: []string{plugin.CapObserver}}
}
func (*followupPanicObserver) Init(context.Context, plugin.Host, plugin.Config) error { return nil }
func (*followupPanicObserver) Start(context.Context) error                            { return nil }
func (*followupPanicObserver) Close(context.Context) error                            { return nil }
func (o *followupPanicObserver) OnEvent(context.Context, plugin.Event) error {
	o.committed.Add(1)
	panic("0020-private-observer-panic")
}

func syncFollowupRoutes(mux *http.ServeMux, n *host.Node) {
	if followupChannel == nil || followupObserver == nil {
		return
	}
	mux.HandleFunc("POST /test/followup/external", func(w http.ResponseWriter, r *http.Request) {
		b := followupChannel
		b.mu.Lock()
		defer b.mu.Unlock()
		if b.channel == "" {
			channel, err := n.Engine().AddChannel(r.Context(), engine.AddChannelRequest{BackendID: "test.externalfollowup", RemoteContext: []byte("0020-private-group"), ReportedName: "0020-private-channel"})
			if err != nil {
				http.Error(w, "external fixture failed", 500)
				return
			}
			b.channel = channel.ID
		}
		if err := b.receiver.Receive(r.Context(), b.channel, plugin.Received{Transfer: "0020-external-transfer", View: "0020-external-view",
			Parts: []plugin.ReceivedPart{{MIME: "text/plain", Data: []byte("0020-external-private-content")}}}); err != nil {
			http.Error(w, "external content declined", http.StatusConflict)
			return
		}
		w.WriteHeader(200)
	})
	mux.HandleFunc("GET /test/followup/observer", func(w http.ResponseWriter, _ *http.Request) {
		_ = json.NewEncoder(w).Encode(map[string]uint64{"committed_callbacks": followupObserver.committed.Load()})
	})
}

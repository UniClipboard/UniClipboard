// Command pluginupdate provides process fixtures for live updates.
package main

import (
	"context"
	"os"
	"strings"

	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/plugin/rpc"
)

var version = "1.0.0"
var capability = "sync_backend"

type state struct{ ownsState bool }
type backend struct{ state }
type action struct{ state }

func (*backend) Spec() plugin.Spec {
	return plugin.Spec{ID: "example.live-update", Version: version, APIVersion: 1,
		Capabilities: []string{plugin.CapSyncBackend}, ExternalChannels: true, TransportKind: plugin.TransportObjectStore}
}
func (b *state) Init(context.Context, plugin.Host, plugin.Config) error {
	lock, err := os.OpenFile("active", os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if err != nil {
		return err
	}
	b.ownsState = true
	return lock.Close()
}
func (*backend) SetReceiver(plugin.Receiver) {}
func (*state) Start(context.Context) error {
	dir, err := os.Getwd()
	if err != nil {
		return err
	}
	if version == "3.0.0" && strings.HasSuffix(dir, ".alias") {
		return rpc.NewError(rpc.CodeUnavailable)
	}
	return nil
}
func (b *state) Close(context.Context) error {
	if b.ownsState {
		b.ownsState = false
		return os.Remove("active")
	}
	return nil
}
func (*backend) Submit(_ context.Context, view plugin.SyncView, host plugin.SyncHost) (bool, error) {
	if err := host.Register(view.Targets); err != nil {
		return false, err
	}
	if version == "1.0.0" {
		return true, nil
	}
	for _, target := range view.Targets {
		host.Report(target, plugin.SyncSent, nil)
	}
	return true, nil
}
func (*backend) PairChannel(context.Context, string) ([]byte, string, error) {
	return []byte("synthetic-group"), "Synthetic group", nil
}

func (*action) Spec() plugin.Spec {
	return plugin.Spec{ID: "example.action-update", Version: version, APIVersion: 1,
		Capabilities: []string{plugin.CapAction},
		Actions: []plugin.ActionSpec{{ID: "show", Title: "Version " + version,
			Accepts: []string{"text"}, Placements: []string{"history.item_menu"}, PostActions: []string{"none"}}}}
}

func (*action) Run(_ context.Context, ctx plugin.ActionContext) error { return ctx.SetText(version) }

func main() {
	var p plugin.Plugin = &backend{}
	if capability == "action" {
		p = &action{}
	}
	if plugin.Serve(p) != nil {
		os.Exit(1)
	}
}

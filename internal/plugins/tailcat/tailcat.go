// Package tailcat is the built-in peer-to-peer sync backend
// (builtin.tailcat). It owns the tailcat transport and the session layer; the
// engine knows neither. The host assembles it explicitly and lists it in
// engine.Options.Plugins as a Builtin instance: nothing registers itself.
//
// It is an ordinary in-process peer_sessions plugin: the Engine drives it
// through the same plugin.PeerBackend path as an out-of-process backend.
package tailcat

import (
	"context"
	"errors"

	"github.com/uniclipboard/unic/internal/session"
	"github.com/uniclipboard/unic/internal/syncdiag"
	"github.com/uniclipboard/unic/internal/tcnet"
	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/plugin/rpc"
)

// ID is the plugin ID of the built-in backend.
const ID = "builtin.tailcat"

// Plugin is the built-in backend.
type Plugin struct {
	observer syncdiag.Observer
	tr       session.Transport // nil: the tailcat transport, created in Init
	dir      string
	b        *session.Backend
}

// New builds the production backend (tailcat transport).
func New() *Plugin { return &Plugin{} }

// NewOn builds the backend on tr. It is how tests supply loopback or
// fault-injecting networks.
func NewOn(tr session.Transport) *Plugin { return &Plugin{tr: tr} }

func (p *Plugin) Spec() plugin.Spec {
	return plugin.Spec{ID: ID, Version: "1", APIVersion: 1, Capabilities: []string{plugin.CapSyncBackend}, TransportKind: plugin.TransportPeerSessions}
}

// SetDir gives the backend its private directory (plugin.DirAware).
func (p *Plugin) SetDir(dir string) { p.dir = dir }

func (p *Plugin) Init(context.Context, plugin.Host, plugin.Config) error {
	if p.tr == nil {
		t, err := tcnet.New(p.dir, func(string, ...any) {}) // tailcat's logs may name addresses
		if err != nil {
			if p.observer != nil {
				func() {
					defer func() { _ = recover() }()
					reason := "unavailable"
					if errors.Is(err, tcnet.ErrBadCredentials) {
						reason = "unusable_credentials"
					}
					p.observer.RecordSync(syncdiag.Record{Epoch: p.observer.SyncEpoch(), Kind: "connection", Phase: "network_start", Direction: "none", Result: "failed", Reason: reason})
				}()
			}
			if errors.Is(err, tcnet.ErrBadCredentials) {
				return rpc.NewError("bad_credentials") // a damaged file is reported, never replaced
			}
			return errors.New("cannot prepare the network")
		}
		p.tr = t
	}
	p.b = session.New(session.Config{Transport: p.tr, Observer: p.observer})
	return nil
}

// Observe installs the host-owned finite observer before Init.
func (p *Plugin) Observe(o syncdiag.Observer) { p.observer = o }
func (p *Plugin) Collect(epoch uint64) ([]syncdiag.Record, bool) {
	if p.b == nil {
		return nil, false
	}
	return p.b.Collect(epoch)
}

func (p *Plugin) ExpectStop() {
	if p.b != nil {
		p.b.ExpectStop()
	}
}

// Start does nothing: the network comes up with the first active state.
func (p *Plugin) Start(context.Context) error { return nil }

func (p *Plugin) Close(ctx context.Context) error {
	if p.b == nil {
		return nil
	}
	return p.b.Close(ctx)
}

func (p *Plugin) SetPeerAuthority(a plugin.PeerAuthority) { p.b.SetPeerAuthority(a) }
func (p *Plugin) SetPeerReceiver(r plugin.PeerReceiver)   { p.b.SetPeerReceiver(r) }

func (p *Plugin) Bind(ctx context.Context, b rpc.Bind) error { return p.b.Bind(ctx, b) }
func (p *Plugin) ApplyState(ctx context.Context, s rpc.SyncState) error {
	return p.b.ApplyState(ctx, s)
}
func (p *Plugin) PairingAddress(ctx context.Context, purpose string) (string, error) {
	return p.b.PairingAddress(ctx, purpose)
}
func (p *Plugin) Connect(ctx context.Context, c rpc.Connect) (rpc.ConnectResult, error) {
	return p.b.Connect(ctx, c)
}
func (p *Plugin) Send(ctx context.Context, s rpc.Send, src plugin.SendSource) error {
	return p.b.Send(ctx, s, src)
}

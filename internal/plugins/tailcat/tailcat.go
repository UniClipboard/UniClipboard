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
func (p *Plugin) SetLinkStatusChanged(fn func(peerID string)) {
	if p.b != nil {
		p.b.SetLinkStatusChanged(fn)
	}
}
func (p *Plugin) SetRelayCallbacks(
	provide func(peerID string) (plugin.RelayProfile, bool),
	onPeerProfile func(peerID string, prof plugin.RelayProfile),
) {
	if p.b != nil {
		p.b.SetRelayCallbacks(provide, onPeerProfile)
	}
}
func (p *Plugin) PushRelayProfiles() {
	if p.b != nil {
		p.b.PushRelayProfiles()
	}
}
func (p *Plugin) WakePeer(peerID string) {
	if p.b != nil {
		p.b.WakePeer(peerID)
	}
}
func (p *Plugin) SetRelayHandoffCallbacks(
	onOffer func(peerID string, offer plugin.RelayHandoffOffer) plugin.RelayHandoffAck,
	onChunk func(peerID, transferID string, index int, ciphertext []byte),
	onPeerConnected func(peerID string, handoffCap, clipAckCap bool),
	onPeerDisconnected func(peerID string),
) {
	if p.b != nil {
		p.b.SetRelayHandoffCallbacks(onOffer, onChunk, onPeerConnected, onPeerDisconnected)
	}
}
func (p *Plugin) PeerHandoffCaps(peerID string) (connected, handoffCap, clipAckCap bool) {
	if p.b == nil {
		return false, false, false
	}
	return p.b.PeerHandoffCaps(peerID)
}
func (p *Plugin) RegisterRelayClipAckWaiter(peerID, transferID string) (<-chan string, <-chan struct{}, func(), bool) {
	if p.b == nil {
		return nil, nil, func() {}, false
	}
	return p.b.RegisterRelayClipAckWaiter(peerID, transferID)
}
func (p *Plugin) SendRelayHandoffOffer(ctx context.Context, peerID string, offer plugin.RelayHandoffOffer) (plugin.RelayHandoffAck, error) {
	if p.b == nil {
		return plugin.RelayHandoffAck{}, rpc.NewError(rpc.CodeUnavailable)
	}
	return p.b.SendRelayHandoffOffer(ctx, peerID, offer)
}
func (p *Plugin) SendRelayChunk(ctx context.Context, peerID, transferID string, index int, ciphertext []byte) error {
	if p.b == nil {
		return rpc.NewError(rpc.CodeUnavailable)
	}
	return p.b.SendRelayChunk(ctx, peerID, transferID, index, ciphertext)
}
func (p *Plugin) SendRelayClipAck(peerID, transferID, status string) bool {
	if p.b == nil {
		return false
	}
	return p.b.SendRelayClipAck(peerID, transferID, status)
}
func (p *Plugin) LinkSnapshots() map[string]rpc.PeerLinkStatus {
	if p.b == nil {
		return nil
	}
	return p.b.LinkSnapshots()
}
func (p *Plugin) ProbePeers(ctx context.Context, peerIDs []string) ([]rpc.PeerLinkStatus, error) {
	if p.b == nil {
		return nil, rpc.NewError(rpc.CodeUnavailable)
	}
	return p.b.ProbePeers(ctx, peerIDs)
}

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
func (p *Plugin) Send(ctx context.Context, s rpc.Send, src plugin.SendSource) (rpc.SendResult, error) {
	return p.b.Send(ctx, s, src)
}

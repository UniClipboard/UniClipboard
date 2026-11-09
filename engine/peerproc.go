package engine

import (
	"context"
	"log/slog"
	"sort"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/uniclipboard/unic/internal/store"
	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/plugin/rpc"
)

// peerProcBackend is an out-of-process peer_sessions sync backend as the
// engine drives it (sync-backend-contract §1–2). The Engine owns identity,
// trust and addresses; this type turns them into the state snapshot the
// backend may act on, and pushes a new revision whenever any of it changes.
// A push returns only once the backend confirmed the state or its process was
// reclaimed, so callers that revoke something may rely on it as the barrier.
type peerProcBackend struct {
	e         *Engine
	id        PluginID
	pb        plugin.PeerBackend
	bindingID string

	pm   sync.Mutex // serializes pushes and guards the rest
	rev  uint64
	last rpc.SyncState // the last state the backend confirmed (Revision zeroed)
	have bool

	am       sync.Mutex // handshakes, attempts, nonces and sessions
	hs       map[string]*handshake
	att      map[string]*attempt
	nonces   map[string]time.Time
	sess     map[string]*session
	q        map[string]*peerQueue
	rx       rxState
	stateRev uint64 // the revision the backend confirmed last
}

func (b *peerProcBackend) ID() PluginID        { return b.id }
func (b *peerProcBackend) Kind() TransportKind { return TransportPeerSessions }

const maxStateName = rpc.MaxPeerNameBytes

// clipName cuts s to at most n bytes on a rune boundary.
func clipName(s string, n int) string {
	if len(s) <= n {
		return s
	}
	s = s[:n]
	for !utf8.ValidString(s) {
		s = s[:len(s)-1]
	}
	return s
}

// bindMsg is the one-time configuration of this backend session.
func (b *peerProcBackend) bindMsg() rpc.Bind {
	return rpc.Bind{BindingID: b.bindingID, SelfID: b.e.ident.DeviceID(), SelfName: clipName(b.e.selfName(), maxStateName),
		WireVersion: rpc.WireVersion, BackendID: string(b.id), TransportKind: string(TransportPeerSessions)}
}

// selfName is the name this device announces.
func (e *Engine) selfName() string {
	e.mu.Lock()
	defer e.mu.Unlock()
	if n := strings.TrimSpace(e.settings.Value.DeviceName); n != "" {
		return n
	}
	return e.defName
}

// snapshot builds the state from the device store and the current settings.
// Revision is left zero.
func (b *peerProcBackend) snapshot() (rpc.SyncState, error) {
	e := b.e
	e.mu.Lock()
	set := e.settings.Value.clone()
	up := e.netUp
	gen := e.netGen
	name := strings.TrimSpace(set.DeviceName)
	if name == "" {
		name = e.defName
	}
	e.mu.Unlock()

	pol, ok := set.BackendPolicies[b.id]
	if !ok {
		pol = DefaultBackendPolicy()
	}
	st := rpc.SyncState{
		BackendID: string(b.id), BindingID: b.bindingID, Runtime: "active", DeviceName: clipName(name, maxStateName),
		SyncEnabled:       set.Sync.Enabled,
		Introduce:         set.Sync.Introductions,
		Policy:            rpc.BackendPolicy{Text: pol.Text, Images: pol.Images, Files: pol.Files, AutoFileMaxBytes: rpc.U64(max(pol.AutoFileMaxBytes, 0))},
		Limits:            rpc.StateLimits{MaxFileBytes: rpc.U64(max(set.Limits.MaxFileBytes, 0)), MaxTotalBytes: rpc.U64(max(set.Limits.MaxTotalBytes, 0))},
		NetworkGeneration: rpc.U64(gen),
		Peers:             []rpc.StatePeer{}, Rejected: []rpc.StateRejected{},
	}
	if !up {
		st.Runtime = "suspended"
	}
	ds, err := e.st.Devices()
	if err != nil {
		return st, err
	}
	for _, d := range ds {
		switch d.Trust {
		case store.TrustTrusted:
			bd, berr := e.st.InstanceBinding(d.ID, string(b.id))
			if berr != nil || bd.State != store.BindingAuthorized || len(bd.Endpoint) == 0 || len(bd.Endpoint) > rpc.MaxPeerAddress {
				continue
			}
			st.Peers = append(st.Peers, rpc.StatePeer{ID: d.ID, Name: clipName(Device{Name: d.Name, ReportedName: d.ReportedName}.DisplayName(), maxStateName),
				Address: string(bd.Endpoint), TrustRevision: rpc.U64(d.Revision + bd.Revision)})
		}
	}
	rs, err := e.st.RevokedDevices()
	if err != nil {
		return st, err
	}
	for _, d := range rs {
		if len(st.Rejected) >= rpc.MaxStateRejected {
			break
		}
		st.Rejected = append(st.Rejected, rpc.StateRejected{ID: d.ID, RequestRevision: rpc.U64(d.Revision)})
	}
	sort.Slice(st.Peers, func(i, j int) bool { return st.Peers[i].ID < st.Peers[j].ID })
	sort.Slice(st.Rejected, func(i, j int) bool { return st.Rejected[i].ID < st.Rejected[j].ID })
	return st, nil
}

// push delivers the current state if it differs from the confirmed one.
func (b *peerProcBackend) push(ctx context.Context) error {
	b.pm.Lock()
	defer b.pm.Unlock()
	st, err := b.snapshot()
	if err != nil {
		return err
	}
	if b.have && st.Equal(b.last) {
		return nil
	}
	next := st
	next.Revision = rpc.U64(b.rev + 1)
	if err := b.pb.ApplyState(ctx, next); err != nil {
		return err
	}
	b.rev++
	b.am.Lock()
	b.stateRev = b.rev
	b.am.Unlock()
	b.last, b.have = st, true
	b.pruneSessions(st)
	return nil
}

const pushTimeout = 30 * time.Second

// syncPeerStates brings every out-of-process peer backend to the current
// state before it returns. A backend that fails is logged by code only; the
// next change retries it, and a revocation is still safe because the proxy
// reclaims a process that does not confirm a state that takes authority away.
func (e *Engine) syncPeerStates() {
	for _, b := range e.peerProcs {
		ctx, cancel := context.WithTimeout(context.Background(), pushTimeout)
		if err := b.push(ctx); err != nil {
			e.logf(slog.LevelWarn, "peer backend state not applied", "backend", string(b.id), "code", string(asError(err).Code))
		}
		cancel()
	}
}

package pluginproc

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"

	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/plugin/rpc"
)

// Peer-sessions backend binding and state delivery (sync-backend-contract §2).
// The Engine's wanted state lives here, so a restarted process is brought to
// the latest state before it is published; ApplyState returns only once the
// state is in force or the process holding the old one is gone (the barrier).

var _ plugin.PeerBackend = peerM{}

type peerM struct{ p *Proxy }

func (m peerM) Bind(ctx context.Context, b rpc.Bind) error            { return m.p.bind(ctx, b) }
func (m peerM) ApplyState(ctx context.Context, s rpc.SyncState) error { return m.p.applyState(ctx, s) }

type peerWant struct {
	bind  *rpc.Bind
	state *rpc.SyncState
}

func (p *Proxy) bind(ctx context.Context, b rpc.Bind) error {
	if err := b.Validate(); err != nil {
		return err
	}
	p.mu.Lock()
	if p.want.bind != nil {
		same := *p.want.bind == b
		p.mu.Unlock()
		if same {
			return nil
		}
		return rpc.NewError(rpc.CodeConflict)
	}
	p.want.bind = &b
	p.mu.Unlock()
	s := p.current()
	if s == nil {
		return nil // replayed when the process comes up
	}
	_, err := s.conn.Call(ctx, rpc.CallOptions{Method: rpc.MethodBind, Timeout: p.callTimeout(ctx, p.opts.ProcessTimeout), Params: b}, nil)
	return err
}

func (p *Proxy) applyState(ctx context.Context, st rpc.SyncState) error {
	if err := st.Validate(); err != nil {
		return err
	}
	select {
	case p.applyGate <- struct{}{}:
		defer func() { <-p.applyGate }()
	case <-ctx.Done():
		return ctx.Err()
	}
	p.mu.Lock()
	if p.want.bind == nil || st.BindingID != p.want.bind.BindingID || st.BackendID != p.want.bind.BackendID {
		p.mu.Unlock()
		return rpc.NewError(rpc.CodeInvalidState)
	}
	if cur := p.want.state; cur != nil && (st.Revision < cur.Revision || (st.Revision == cur.Revision && !st.Equal(*cur))) {
		p.mu.Unlock()
		return rpc.NewError(rpc.CodeConflict)
	}
	prev := p.want.state
	p.want.state = &st
	p.mu.Unlock()
	s := p.current()
	if s == nil {
		return nil // no process holds any state; the restart replays the newest
	}
	err := p.sendState(ctx, s, st)
	if err == nil {
		return nil
	}
	if isRejection(err) && (prev == nil || !revokes(*prev, st)) {
		// The backend answered that it kept its previous snapshot, and nothing
		// in the new one takes anything away, so the old state is still a
		// correct one. Report the refusal and do not replay the refused state.
		p.mu.Lock()
		if p.want.state == &st {
			p.want.state = prev
		}
		p.mu.Unlock()
		return err
	}
	// No answer, a broken answer, or a refusal of a state that revokes
	// something: the old state may still be in force inside that process, so
	// the only way to guarantee revocation is to reclaim it. Restart replays.
	p.terminate(s)
	return nil
}

// isRejection reports a definite "not installed, previous snapshot kept"
// answer from the backend, as opposed to a timeout, crash or protocol fault.
func isRejection(err error) bool {
	var re *rpc.Error
	if !errors.As(err, &re) {
		return false
	}
	switch re.Code {
	case rpc.CodeInvalidArgument, rpc.CodeConflict, rpc.CodeLimitExceeded, rpc.CodeExhausted, rpc.CodeInvalidState:
		return true
	}
	return false
}

// revokes reports whether moving from old to next removes any authority the
// backend held: a peer dropped or changed, an identity newly rejected,
// suspension or sync turned off, a type policy or limit tightened, or the
// network generation moved (existing sessions must go).
func revokes(old, next rpc.SyncState) bool {
	np := make(map[string]rpc.StatePeer, len(next.Peers))
	for _, p := range next.Peers {
		np[p.ID] = p
	}
	for _, p := range old.Peers {
		if q, ok := np[p.ID]; !ok || q.Address != p.Address || q.TrustRevision != p.TrustRevision {
			return true
		}
	}
	or := make(map[string]bool, len(old.Rejected))
	for _, r := range old.Rejected {
		or[r.ID] = true
	}
	for _, r := range next.Rejected {
		if !or[r.ID] {
			return true
		}
	}
	a, b := old.Policy, next.Policy
	return (old.Runtime == "active" && next.Runtime != "active") ||
		(old.SyncEnabled && !next.SyncEnabled) ||
		(a.Text && !b.Text) || (a.Images && !b.Images) || (a.Files && !b.Files) ||
		b.AutoFileMaxBytes < a.AutoFileMaxBytes ||
		next.Limits.MaxFileBytes < old.Limits.MaxFileBytes || next.Limits.MaxTotalBytes < old.Limits.MaxTotalBytes ||
		next.NetworkGeneration != old.NetworkGeneration
}

func (p *Proxy) sendState(ctx context.Context, s *session, st rpc.SyncState) error {
	raw, err := json.Marshal(st)
	if err != nil || len(raw) == 0 || len(raw) > rpc.MaxStateBytes {
		return rpc.NewError(rpc.CodeLimitExceeded)
	}
	var res rpc.Revision
	id, err := s.conn.Call(ctx, rpc.CallOptions{Method: rpc.MethodApplyState, Timeout: p.callTimeout(ctx, p.opts.ProcessTimeout),
		Build: func(id uint64) (any, error) {
			ss, d, err := s.conn.NewSend(s.conn.Context(), id, rpc.PurposeSyncState, uint64(len(raw)))
			if err != nil {
				return nil, err
			}
			go func() {
				if _, err := io.Copy(ss, bytes.NewReader(raw)); err == nil {
					_ = ss.Close()
				}
			}()
			return rpc.ApplyState{Revision: st.Revision, Snapshot: d}, nil
		}}, &res)
	if err != nil {
		return err
	}
	s.conn.ReleaseRoot(id)
	if res.Revision != st.Revision {
		return rpc.NewError(rpc.CodeInternal)
	}
	return nil
}

type wantKey struct {
	bound bool
	rev   uint64
}

func (p *Proxy) wantKeyLocked() wantKey {
	k := wantKey{bound: p.want.bind != nil}
	if p.want.state != nil {
		k.rev = uint64(p.want.state.Revision)
	}
	return k
}

// replay brings a fresh, unpublished session to the wanted state and reports
// what it delivered so the caller can detect a newer want.
func (p *Proxy) replay(ctx context.Context, s *session) (wantKey, error) {
	p.mu.Lock()
	k, b, st := p.wantKeyLocked(), p.want.bind, p.want.state
	p.mu.Unlock()
	if b == nil {
		return k, nil
	}
	if _, err := s.conn.Call(ctx, rpc.CallOptions{Method: rpc.MethodBind, Timeout: p.opts.StartTimeout, Params: *b}, nil); err != nil {
		return k, err
	}
	if st == nil {
		return k, nil
	}
	return k, p.sendState(ctx, s, *st)
}

// launch starts a process, runs the handshake and Start, and replays the
// wanted state until it is current. It always returns with p.mu held, so the
// caller can publish atomically: a state that arrived during replay is never
// missed, because ApplyState stores it before looking for a live session.
func (p *Proxy) launch(ctx context.Context) (*session, error) {
	s, err := p.spawn(ctx)
	if err == nil {
		err = p.handshakeCalls(ctx, s, p.cfg)
		for err == nil {
			var k wantKey
			if k, err = p.replay(ctx, s); err != nil {
				break
			}
			p.mu.Lock()
			if p.wantKeyLocked() == k {
				// Start only after the state is installed, so the backend
				// never runs with less than the Engine wants.
				if _, err = s.conn.Call(ctx, rpc.CallOptions{Method: rpc.MethodStart, Timeout: p.opts.StartTimeout, Params: rpc.Empty{}}, nil); err == nil {
					return s, nil
				}
				p.mu.Unlock()
				break
			}
			p.mu.Unlock()
		}
		p.terminate(s)
	}
	p.mu.Lock()
	return nil, err
}

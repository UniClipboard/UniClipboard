package engine

import (
	"bytes"
	"context"
	"crypto/subtle"
	"io"
	"slices"
	"sort"
	"sync"
	"time"

	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/plugin/rpc"
)

// Sending through an out-of-process peer_sessions backend (sync-backend
// contract §4). The Engine picks the connected peers, hands each one a
// backend.send of its own and answers authorize_send / open_send_part from the
// committed view. Per peer there is one send in flight and at most one waiting;
// a newer view replaces the waiting one.

type peerJob struct {
	run       func(context.Context)
	supersede func()
}

type peerQueue struct {
	busy    bool
	pending *peerJob
}

// enqueue runs j now, or parks it behind the send in flight (replacing, and
// superseding, an older parked one).
func (b *peerProcBackend) enqueue(ctx context.Context, peer string, j *peerJob) bool {
	b.am.Lock()
	if b.q == nil {
		b.q = map[string]*peerQueue{}
	}
	q := b.q[peer]
	if q == nil {
		q = &peerQueue{}
		b.q[peer] = q
	}
	if q.busy {
		old := q.pending
		q.pending = j
		b.am.Unlock()
		if old != nil {
			old.supersede()
		}
		return true
	}
	if !b.e.startPeerWork() { // admitted together with Stop, which waits for it
		if len(b.q) > 0 && !q.busy {
			delete(b.q, peer)
		}
		b.am.Unlock()
		return false
	}
	q.busy = true
	b.am.Unlock()
	go func() {
		defer b.e.opWG.Done()
		for j != nil {
			j.run(ctx)
			b.am.Lock()
			j, q.pending = q.pending, nil
			if j == nil {
				q.busy = false
				delete(b.q, peer)
			}
			b.am.Unlock()
		}
	}()
	return true
}

// startPeerWork admits a background send; it fails once the engine stops.
func (e *Engine) startPeerWork() bool {
	e.mu.Lock()
	defer e.mu.Unlock()
	if e.life != LifecycleRunning {
		return false
	}
	e.opWG.Add(1)
	return true
}

// liveTargets are the sessions that may be sent to right now, one per peer.
func (b *peerProcBackend) liveTargets() []session {
	b.am.Lock()
	var cand []session
	for _, s := range b.sess {
		if s.connected && s.ended.IsZero() {
			cand = append(cand, *s)
		}
	}
	b.am.Unlock()
	var out []session
	seen := map[string]bool{}
	sort.Slice(cand, func(i, j int) bool { return cand[i].conn < cand[j].conn })
	for _, s := range cand {
		if rev, ok := b.trustRevision(s.peerID); !ok || rev != s.trustRev || seen[s.peerID] {
			continue
		}
		seen[s.peerID] = true
		out = append(out, s)
	}
	return out
}

func rpcFromErr(err error) error {
	if err == nil {
		return nil
	}
	if re, ok := err.(*rpc.Error); ok {
		return re
	}
	return rpc.NewError(string(asError(err).Code))
}

func (b *peerProcBackend) clipOf(v backendView) rpc.Clip {
	c := rpc.Clip{WireID: v.id, Origin: b.e.ident.DeviceID(), AtMs: rpc.U64(max(v.tsMs, 0)), Hash: v.set.hash}
	for i, p := range v.set.parts {
		cp := rpc.ClipPart{Index: i, Size: rpc.U64(p.desc.Size)}
		if p.desc.Kind == PartFile {
			cp.Kind, cp.Name = "file", p.desc.Name
		} else {
			cp.Kind, cp.MIME = "representation", p.desc.MIME
		}
		c.Parts = append(c.Parts, cp)
	}
	return c
}

// Submit queues the view for every connected peer without waiting for the network.
func (b *peerProcBackend) Submit(ctx context.Context, v backendView, h backendHost) (bool, *Error) {
	e := b.e
	disp, ok := b.pb.(plugin.PeerDispatcher)
	if !ok {
		return false, newErr(CodeUnsupported, "the backend cannot send")
	}
	e.mu.Lock()
	rerr := e.needRunningLocked()
	paused := !e.settings.Value.Sync.Enabled
	e.mu.Unlock()
	switch {
	case ctx.Err() != nil:
		return false, ctxErr(ctx.Err())
	case rerr != nil:
		return false, rerr
	case paused:
		return false, newErr(CodeSyncDisabled, "sync is paused")
	}
	targets := b.liveTargets()
	if len(v.devices) > 0 {
		kept := targets[:0]
		for _, s := range targets {
			if slices.Contains(v.devices, DeviceID(s.peerID)) {
				kept = append(kept, s)
			}
		}
		targets = kept
	}
	if len(targets) == 0 {
		return false, nil
	}
	keys := make([]targetKey, len(targets))
	ctxSubmission := v.workContext
	for i, s := range targets {
		keys[i] = targetKey{b.id, TargetDevice, s.peerID}
	}
	if rerr := h.Register(keys); rerr != nil {
		return false, rerr
	}
	clip := b.clipOf(v)
	e.mu.Lock()
	rc := e.sendCtx
	e.mu.Unlock()
	for i, s := range targets {
		k, s := keys[i], s
		queued := time.Now()
		kind := diagnosticKind(v.set.c)
		backend := diagnosticBackend(b.id)
		emitQueue := func(result DiagnosticResult) {
			code := Code("")
			if result == "cancelled" {
				code = CodeCancelled
			}
			e.diagnostic(context.Background(), DiagnosticRecord{Stage: "send_queue", Result: result, Code: code, Backend: backend, ContentKind: kind, Duration: time.Since(queued)})
		}
		job := &peerJob{
			supersede: func() { h.Report(k, TargetSuperseded, nil); emitQueue("superseded") },
			run: func(sendCtx context.Context) {
				ctx, stop := context.WithCancel(sendCtx)
				defer stop()
				if ctxSubmission != nil {
					unhook := context.AfterFunc(ctxSubmission, stop)
					defer unhook()
				}
				if ctx.Err() != nil {
					h.Report(k, TargetFailed, ctx.Err())
					emitQueue("cancelled")
					return
				}
				emitQueue("started")
				h.Report(k, TargetSending, nil)
				// A duplicate connection to the same peer is resolved shortly
				// after both ends connect: the loser ends before its first byte.
				// A send that was refused before authorization moves to the
				// surviving session instead of failing.
				cur := s
				for attempt := 0; ; attempt++ {
					src := &peerSendSource{b: b, v: v, conn: cur.conn, token: cur.token}
					started := time.Now()
					err := b.sendOver(ctx, disp, src, cur, v, clip)
					outcome, code := diagnosticOutcome(err)
					if ctx.Err() != nil {
						outcome = "cancelled"
						code = CodeCancelled
					}
					e.diagnostic(ctx, DiagnosticRecord{Stage: "send", Result: outcome, Backend: backend, ContentKind: kind, Code: code, Duration: time.Since(started)})
					unsent := !src.authorized()
					src.close()
					if err == nil {
						h.Report(k, TargetSent, nil)
						return
					}
					next, ok := b.survivor(cur)
					if !unsent || !ok || attempt >= 2 {
						h.Report(k, TargetFailed, err)
						return
					}
					cur = next
				}
			},
		}
		if !b.enqueue(rc, s.peerID, job) {
			h.Report(k, TargetFailed, errClosed)
			emitQueue("cancelled")
		}
	}
	return true, nil
}

// sendOver dispatches the clip over one session.
func (b *peerProcBackend) sendOver(ctx context.Context, disp plugin.PeerDispatcher, src *peerSendSource, s session, v backendView, clip rpc.Clip) error {
	msg := rpc.Send{DispatchID: randHex(16), OperationID: v.id, PeerID: s.peerID, ConnectionID: s.conn, SessionToken: s.token,
		ContentHandle: randHex(32), Clip: clip, BackendID: string(b.id), Mode: "automatic"}
	if err := msg.Validate(); err != nil {
		return err
	}
	return disp.Send(ctx, msg, src)
}

// survivor is the live session that replaced old for the same peer, if any.
func (b *peerProcBackend) survivor(old session) (session, bool) {
	for _, t := range b.liveTargets() {
		if t.peerID == old.peerID && t.conn != old.conn {
			return t, true
		}
	}
	return session{}, false
}

// authorized reports whether the backend was ever granted the right to send.
func (s *peerSendSource) authorized() bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.grant != ""
}

// peerSendSource answers the backend's authorize/open requests for one send.
type peerSendSource struct {
	b     *peerProcBackend
	v     backendView
	conn  string
	token string

	mu      sync.Mutex
	grant   string
	handles []io.ReadCloser // opened at authorization, one per part
}

func (s *peerSendSource) close() {
	s.mu.Lock()
	hs := s.handles
	s.handles = nil
	s.mu.Unlock()
	for _, h := range hs {
		if h != nil {
			_ = h.Close()
		}
	}
}

// Authorize is the last check before the first wire byte: the connection must
// still be the one the send was addressed to and the peer still approved, and
// the global switches, policy and limits must still allow every part.
func (s *peerSendSource) Authorize(ctx context.Context, a rpc.Authorize) (rpc.SendGrant, error) {
	b, e := s.b, s.b.e
	if subtle.ConstantTimeCompare([]byte(a.ConnectionID), []byte(s.conn)) != 1 || subtle.ConstantTimeCompare([]byte(a.SessionToken), []byte(s.token)) != 1 {
		return rpc.SendGrant{}, rpc.NewError(rpc.CodePermission)
	}
	b.am.Lock()
	cur := b.sess[s.conn]
	live := cur != nil && cur.connected && cur.ended.IsZero()
	var peer string
	var rev uint64
	if live {
		peer, rev = cur.peerID, cur.trustRev
	}
	stateRev := b.stateRev
	b.am.Unlock()
	if tr, ok := b.trustRevision(peer); !live || !ok || tr != rev {
		return rpc.SendGrant{}, rpc.NewError(rpc.CodePermission)
	}
	if !b.syncEnabled(peer) {
		return rpc.SendGrant{}, rpc.NewError("sync_disabled")
	}
	e.mu.Lock()
	rerr := e.needRunningLocked()
	enabled, lim := e.settings.Value.Sync.Enabled, e.settings.Value.Limits
	e.mu.Unlock()
	if rerr != nil {
		return rpc.SendGrant{}, rpcFromErr(rerr)
	}
	if !enabled {
		return rpc.SendGrant{}, rpc.NewError("sync_disabled")
	}
	sy := e.policyOf(b.id)
	for _, p := range s.v.set.parts {
		if !syncAllowsPart(p, sy) {
			return rpc.SendGrant{}, rpc.NewError("sync_disabled")
		}
	}
	if lerr := checkLimits(s.v.set, lim); lerr != nil {
		return rpc.SendGrant{}, rpcFromErr(lerr)
	}
	// Every referenced file is opened now, so a change cannot be found out
	// after the header went out.
	hs := make([]io.ReadCloser, len(s.v.set.parts))
	for i, p := range s.v.set.parts {
		if p.data != nil {
			hs[i] = io.NopCloser(bytes.NewReader(p.data))
			continue
		}
		octx, cancel := context.WithTimeout(ctx, 30*time.Second)
		rc, oerr := e.openPart(octx, p)
		cancel()
		if oerr != nil {
			for _, h := range hs {
				if h != nil {
					_ = h.Close()
				}
			}
			return rpc.SendGrant{}, rpcFromErr(oerr)
		}
		hs[i] = rc
	}
	s.mu.Lock()
	if s.grant != "" {
		s.mu.Unlock()
		for _, h := range hs {
			_ = h.Close()
		}
		return rpc.SendGrant{}, rpc.NewError(rpc.CodeConflict)
	}
	s.grant, s.handles = randHex(32), hs
	g := s.grant
	s.mu.Unlock()
	return rpc.SendGrant{GrantID: g, StateRevision: rpc.U64(stateRev)}, nil
}

// OpenPart hands out one part's reader, once.
func (s *peerSendSource) OpenPart(_ context.Context, grantID string, index int) (io.ReadCloser, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.grant == "" || subtle.ConstantTimeCompare([]byte(grantID), []byte(s.grant)) != 1 || index < 0 || index >= len(s.handles) || s.handles[index] == nil {
		return nil, rpc.NewError(rpc.CodePermission)
	}
	h := s.handles[index]
	s.handles[index] = nil
	return h, nil
}

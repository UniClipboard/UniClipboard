package engine

import (
	"context"
	"crypto/sha256"
	"sort"
	"time"

	"github.com/uniclipboard/unic/internal/peerbackend"
	"github.com/uniclipboard/unic/internal/wire"
)

// This file is the multi-backend sync boundary of 0007. Engine processes and
// commits a clip once, then offers the same final content to every enabled
// backend, each through its own policy. A backend decides nothing about
// authorization: it only reports which targets accepted work and how each
// target fares. The only production backend is still tailcat (below); the
// interface is private until the plugin RPC of P4 can carry it.

// BackendTailcat is the ID of the built-in peer-to-peer backend.
const BackendTailcat PluginID = "builtin.tailcat"

type TransportKind string

const (
	TransportPeerSessions TransportKind = "peer_sessions"
	TransportObjectStore  TransportKind = "object_store"
)

type SyncTargetKind string

const (
	TargetDevice     SyncTargetKind = "device"
	TargetRepository SyncTargetKind = "repository"
	TargetGroup      SyncTargetKind = "group" // an external channel (0009)
)

type BackendDisposition string

const (
	BackendNotRequested     BackendDisposition = "not_requested"
	BackendDisabled         BackendDisposition = "disabled"
	BackendPolicyFiltered   BackendDisposition = "policy_filtered"
	BackendAutoFileTooLarge BackendDisposition = "auto_file_too_large"
	BackendNoTargets        BackendDisposition = "no_targets"
	BackendQueued           BackendDisposition = "queued"
	BackendFailed           BackendDisposition = "failed"
)

// BackendSubmissionResult is one backend's decision for a commit.
type BackendSubmissionResult struct {
	BackendID   PluginID           `json:"backend_id"`
	Disposition BackendDisposition `json:"disposition"`
	Error       *Error             `json:"error,omitempty"`
}

// targetKey names one target of one backend. id is public and opaque.
type targetKey struct {
	backend PluginID
	kind    SyncTargetKind
	id      string
}

// backendView is what one backend is asked to send: the final content
// filtered by that backend's policy. id and tsMs are shared by all backends
// of a commit (the transfer identity of 0007 §6).
type backendView struct {
	set  sendSet
	id   string
	tsMs int64
}

// backendHost is handed to a backend for one submission.
type backendHost interface {
	// Register announces the targets about to receive work. It must be called
	// before any of them can start, and creates or joins the operation.
	Register(targets []targetKey) *Error
	// Report tells how a registered target fares; cause may be any error.
	Report(k targetKey, st TargetState, cause error)
}

type syncBackend interface {
	ID() PluginID
	Kind() TransportKind
	// Submit queues the view for the backend's targets without waiting for the
	// network. queued=false with a nil error means there was nobody to send to.
	Submit(ctx context.Context, v backendView, h backendHost) (queued bool, err *Error)
}

// ---- tailcat ----

type tailcatBackend struct{ e *Engine }

func (tailcatBackend) ID() PluginID        { return BackendTailcat }
func (tailcatBackend) Kind() TransportKind { return TransportPeerSessions }

func (b tailcatBackend) Submit(ctx context.Context, v backendView, h backendHost) (bool, *Error) {
	e := b.e
	var failure *Error
	queued := e.core.Submit(func(peers []string) (peerbackend.Job, bool) {
		e.mu.Lock()
		err := e.needNetworkLocked()
		paused := !e.settings.Value.Sync.Enabled
		e.mu.Unlock()
		switch {
		case ctx.Err() != nil:
			failure = ctxErr(ctx.Err())
			return peerbackend.Job{}, false
		case err != nil:
			failure = err
			return peerbackend.Job{}, false
		case paused:
			failure = newErr(CodeSyncDisabled, "sync is paused")
			return peerbackend.Job{}, false
		}
		keys := make([]targetKey, len(peers))
		for i, p := range peers {
			keys[i] = targetKey{BackendTailcat, TargetDevice, p}
		}
		if rerr := h.Register(keys); rerr != nil {
			failure = rerr
			return peerbackend.Job{}, false
		}
		return peerbackend.Job{
			Report: func(peer string, st peerbackend.TargetState, cause error) {
				h.Report(targetKey{BackendTailcat, TargetDevice, peer}, targetStateOf(st), cause)
			},
			Send: func(ctx context.Context, w *wire.Conn, origin string) (bool, error) {
				return e.sendSet(ctx, w, origin, v.id, v.tsMs, v.set, func() BackendSyncPolicy { return e.policyOf(BackendTailcat) })
			},
		}, true
	})
	if !queued {
		return false, failure
	}
	return true, nil
}

func targetStateOf(st peerbackend.TargetState) TargetState {
	switch st {
	case peerbackend.Sending:
		return TargetSending
	case peerbackend.Sent:
		return TargetSent
	case peerbackend.Superseded:
		return TargetSuperseded
	case peerbackend.Failed:
		return TargetFailed
	}
	return TargetQueued
}

// ---- dispatch ----

// dispatchHost is the backendHost of one commit. All backends of the commit
// share it, so their targets end up in one operation.
type dispatchHost struct {
	e    *Engine
	req  SubmitRequest
	hash [sha256.Size]byte
	c    *contentObj
	op   *opEntry // set by the first Register; guarded by e.mu
}

func (h *dispatchHost) Register(keys []targetKey) *Error {
	e := h.e
	e.mu.Lock()
	defer e.mu.Unlock()
	if h.op == nil {
		o, _, err := e.newOpLocked(OperationSend, h.req.RequestID, h.hash, PhaseQueued)
		if err != nil {
			return err
		}
		o.content, o.open = h.c, true // the operation takes over the dispatch reference
		h.op = o
	}
	for _, k := range keys {
		ts := TargetStatus{BackendID: k.backend, TargetKind: k.kind, TargetID: k.id, State: TargetQueued}
		if k.kind == TargetDevice {
			ts.PeerID = DeviceID(k.id)
		}
		h.op.op.Targets = append(h.op.op.Targets, ts)
	}
	e.commitObjsLocked([]ObjectRef{{Type: "operation", ID: string(h.op.op.ID)}}, EventOperations)
	return nil
}

func (h *dispatchHost) Report(k targetKey, st TargetState, cause error) {
	if h.op != nil {
		h.e.reportTarget(h.op.op.ID, k, st, cause)
	}
}

// finish ends the registration window: from now on an operation whose targets
// are all final may complete.
func (h *dispatchHost) finish() {
	e := h.e
	e.mu.Lock()
	o := h.op
	if o == nil {
		e.mu.Unlock()
		return
	}
	o.open = false
	done := e.refreshOpLocked(o)
	e.mu.Unlock()
	if done != nil {
		e.cs.unref(done)
	}
}

// refreshOpLocked recomputes phase and outcome and returns the content to
// release when the operation just completed. e.mu held.
func (e *Engine) refreshOpLocked(o *opEntry) (done *contentObj) {
	ph := phaseOf(o.op.Targets)
	if o.open && ph == PhaseCompleted {
		ph = PhaseSending // more targets may still register
	}
	o.op.Phase, o.op.Outcome = ph, outcomeOf(o.op.Targets)
	if o.open {
		o.op.Outcome = ""
	}
	o.op.UpdatedAtMs = time.Now().UnixMilli()
	if o.op.Phase == PhaseCompleted && o.content != nil {
		done, o.content = o.content, nil
	}
	e.commitObjsLocked([]ObjectRef{{Type: "operation", ID: string(o.op.ID)}}, EventOperations)
	return done
}

func (e *Engine) selectBackends(ids []PluginID) []syncBackend {
	var out []syncBackend
	for _, b := range e.syncBackends {
		if len(ids) == 0 || containsID(ids, b.ID()) {
			out = append(out, b)
		}
	}
	sort.Slice(out, func(i, j int) bool { return out[i].ID() < out[j].ID() })
	return out
}

func containsID(ids []PluginID, id PluginID) bool {
	for _, x := range ids {
		if x == id {
			return true
		}
	}
	return false
}

func fileTooLarge(set sendSet, max int64) bool {
	if max <= 0 {
		return false
	}
	for _, p := range set.parts {
		if p.desc.Kind == PartFile && p.desc.Size > max {
			return true
		}
	}
	return false
}

// dispatchProcessed offers the processed content to the selected backends.
// A non-nil commitFail means nothing was committed (stopped, suspended,
// cancelled). Otherwise the per-backend results and the aggregate follow
// 0007 §4: any queued backend makes the commit queued; else the first of
// disabled, not requested, no backend, failed, no targets, policy filtered.
func (e *Engine) dispatchProcessed(ctx context.Context, req SubmitRequest, hash [sha256.Size]byte, processed *processedContent) (id OperationID, disp SyncDisposition, results []BackendSubmissionResult, syncErr, commitFail *Error) {
	e.mu.Lock()
	if err := ctx.Err(); err != nil {
		e.mu.Unlock()
		return "", "", nil, nil, ctxErr(err)
	}
	if err := e.needRunningLocked(); err != nil {
		e.mu.Unlock()
		return "", "", nil, nil, err
	}
	enabled, lim := e.settings.Value.Sync.Enabled, e.settings.Value.Limits
	e.mu.Unlock()

	backends := e.selectBackends(req.BackendIDs)
	all := func(d BackendDisposition) []BackendSubmissionResult {
		out := make([]BackendSubmissionResult, len(backends))
		for i, b := range backends {
			out[i] = BackendSubmissionResult{BackendID: b.ID(), Disposition: d}
		}
		return out
	}
	switch {
	case !enabled:
		return "", SyncDisabled, all(BackendDisabled), nil, nil
	case !processed.policy.Sync:
		return "", SyncNotRequested, all(BackendNotRequested), nil, nil
	case len(backends) == 0:
		return "", SyncNoBackend, nil, nil, nil
	}

	c := processed.c
	cs := e.cs
	cs.mu.Lock()
	c.refs++ // dispatch owns a reference until an operation takes it over
	cs.mu.Unlock()
	host := &dispatchHost{e: e, req: req, hash: hash, c: c}
	transfer, tsMs := wire.NewID(), time.Now().UnixMilli()
	manual := req.Origin == OriginExplicit

	for _, b := range backends {
		res := BackendSubmissionResult{BackendID: b.ID()}
		pol := e.policyOf(b.ID())
		set := buildSendSet(c, pol)
		switch {
		case len(set.parts) == 0:
			res.Disposition = BackendPolicyFiltered
		case !manual && fileTooLarge(set, pol.AutoFileMaxBytes):
			res.Disposition = BackendAutoFileTooLarge
		default:
			if lerr := checkLimits(set, lim); lerr != nil {
				res.Disposition, res.Error = BackendFailed, lerr
				break
			}
			queued, err := b.Submit(ctx, backendView{set: set, id: transfer, tsMs: tsMs}, host)
			switch {
			case queued:
				res.Disposition = BackendQueued
			case err != nil && err.Code == CodeSyncDisabled:
				res.Disposition = BackendDisabled
			case err != nil:
				res.Disposition, res.Error = BackendFailed, err
			default:
				res.Disposition = BackendNoTargets
			}
		}
		results = append(results, res)
	}
	host.finish()
	if host.op == nil {
		cs.unref(c) // nothing took the reference over
	}
	return e.aggregate(host, results)
}

func (e *Engine) aggregate(h *dispatchHost, results []BackendSubmissionResult) (OperationID, SyncDisposition, []BackendSubmissionResult, *Error, *Error) {
	has := func(d BackendDisposition) bool {
		for _, r := range results {
			if r.Disposition == d {
				return true
			}
		}
		return false
	}
	switch {
	case has(BackendQueued):
		e.mu.Lock()
		id := h.op.op.ID
		e.mu.Unlock()
		return id, SyncQueued, results, nil, nil
	case has(BackendFailed):
		for _, r := range results {
			if r.Disposition == BackendFailed {
				return "", SyncFailed, results, cloneError(r.Error), nil
			}
		}
	case has(BackendDisabled):
		return "", SyncDisabled, results, nil, nil
	case has(BackendNoTargets):
		return "", SyncNoTargets, results, nil, nil
	}
	return "", SyncPolicyFilter, results, nil, nil
}

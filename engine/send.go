package engine

import (
	"bytes"
	"context"
	"crypto/sha256"
	"errors"
	"io"
	"time"

	"github.com/uniclipboard/unic/internal/clip"
	"github.com/uniclipboard/unic/internal/session"
	"github.com/uniclipboard/unic/internal/wire"
)

// sendSet is what will go out: the parts of a content that the sync settings
// allow, fixed at submission.
type sendSet struct {
	c     *contentObj
	parts []*partRec
	hash  string
	total int64
}

// buildSendSet applies the sync filters (0002) to a content.
func buildSendSet(c *contentObj, sy SyncSettings) sendSet {
	set := sendSet{c: c}
	var snap clip.Snapshot
	for _, p := range c.parts {
		switch {
		case p.desc.Kind == PartFile:
			if !sy.Files {
				continue
			}
			snap.Files = append(snap.Files, clip.File{Name: p.desc.Name, Size: p.desc.Size})
		case p.desc.MIME == MimePNG:
			if !sy.Images {
				continue
			}
			snap.Parts = append(snap.Parts, clip.Part{Format: p.desc.MIME, Data: p.data})
		default:
			if !sy.Text {
				continue
			}
			snap.Parts = append(snap.Parts, clip.Part{Format: p.desc.MIME, Data: p.data})
		}
		set.parts = append(set.parts, p)
		set.total += p.desc.Size
	}
	// Receivers compute this hash from what they get and drop a clip that differs.
	set.hash = snap.Hash()
	return set
}

// SubmitContent sends leased content to the devices connected right now.
// Accepting the request only means the engine took it over: the operation
// tells, per device, whether it was sent, failed, or replaced by newer
// content. "Sent" means every byte was written to the connection, nothing
// more. Nothing is queued for devices that are offline, and nothing is
// resent after a reconnect.
func (e *Engine) SubmitContent(ctx context.Context, req SubmitRequest) (OperationID, error) {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return "", gerr
	}
	defer release()
	if req.Origin != OriginExplicit {
		return "", newErr(CodeInvalidArgument, "only explicit submissions are accepted; the clipboard path goes through ObserveClipboard")
	}
	return e.submit(req)
}

func (e *Engine) submit(req SubmitRequest) (OperationID, error) {
	if req.RequestID == "" {
		return "", newErr(CodeInvalidArgument, "RequestID is required")
	}
	hash := sha256.Sum256([]byte("submit\x00" + string(req.LeaseID) + "\x00" + string(req.Origin)))

	e.mu.Lock()
	// A repeated request is recognized before anything else, so it keeps its
	// answer even if the lease has been released since.
	if old := e.ops.reqs[req.RequestID]; old != nil {
		defer e.mu.Unlock()
		if old.op.Kind != OperationSend || old.hash != hash {
			return "", newErr(CodeConflict, "this RequestID was used for a different request")
		}
		return old.op.ID, nil
	}
	if nerr := e.needNetworkLocked(); nerr != nil {
		e.mu.Unlock()
		return "", nerr
	}
	sy, lim := e.settings.Value.Sync, e.settings.Value.Limits
	e.mu.Unlock()
	if !sy.Enabled {
		return "", newErr(CodeSyncDisabled, "sync is paused")
	}

	cs := e.cs
	cs.mu.Lock()
	l := cs.leases[req.LeaseID]
	if l == nil {
		defer cs.mu.Unlock()
		if e.foreign(string(req.LeaseID)) || cs.gone.has(string(req.LeaseID)) {
			return "", newErr(CodeResourceExpired, "the lease is no longer valid")
		}
		return "", newErr(CodeNotFound, "no such lease")
	}
	c := l.c
	c.refs++ // the operation's reference; dropped when it finishes or is not created
	cs.mu.Unlock()
	abandon := func() { cs.unref(c) }

	set := buildSendSet(c, sy)
	if len(set.parts) == 0 {
		abandon()
		return "", newErr(CodeSyncDisabled, "none of this content is enabled for sync")
	}
	if lerr := checkLimits(set, lim); lerr != nil {
		abandon()
		return "", lerr
	}

	var opID OperationID
	var failure *Error
	queued := e.core.Submit(func(peers []string) (session.Job, bool) {
		e.mu.Lock()
		defer e.mu.Unlock()
		o, _, oerr := e.newOpLocked(OperationSend, req.RequestID, hash, PhaseQueued)
		if oerr != nil {
			failure = oerr
			return session.Job{}, false
		}
		opID = o.op.ID
		o.op.Targets = make([]TargetStatus, len(peers))
		for i, p := range peers {
			o.op.Targets[i] = TargetStatus{PeerID: DeviceID(p), State: TargetQueued}
		}
		o.content = c
		clipID, ts := wire.NewID(), time.Now().UnixMilli()
		return session.Job{
			Report: func(peer string, st session.TargetState, err error) { e.reportTarget(opID, DeviceID(peer), st, err) },
			Send: func(ctx context.Context, w *wire.Conn, origin string) (bool, error) {
				return e.sendSet(ctx, w, origin, clipID, ts, set)
			},
		}, true
	})
	if !queued {
		abandon()
		if failure != nil {
			return "", failure
		}
		return "", newErr(CodeNoTargets, "no paired device is connected")
	}
	return opID, nil
}

func checkLimits(set sendSet, lim Limits) *Error {
	if set.total > lim.MaxTotalBytes {
		return newErr(CodeLimitExceeded, "the content is over the configured size limit")
	}
	for _, p := range set.parts {
		if p.desc.Kind == PartFile && p.desc.Size > lim.MaxFileBytes {
			return newErr(CodeLimitExceeded, "a file is over the configured size limit")
		}
	}
	return nil
}

// sendSet writes the content to w. It runs on the session's writer, once per
// device. Everything that can fail without touching the stream (settings,
// files that changed) is checked before the first byte is written.
func (e *Engine) sendSet(ctx context.Context, w *wire.Conn, origin, id string, ts int64, set sendSet) (broken bool, err error) {
	e.mu.Lock()
	sy, lim := e.settings.Value.Sync, e.settings.Value.Limits
	e.mu.Unlock()
	if !sy.Enabled {
		return false, newErr(CodeSyncDisabled, "sync was paused before the transfer started")
	}
	if lerr := checkLimits(set, lim); lerr != nil {
		return false, lerr
	}
	ctx, cancel := context.WithTimeout(ctx, 30*time.Second) // opening files only; the transfer itself is bounded by the connection
	defer cancel()
	var out []wire.OutPart
	var open []io.Closer
	defer func() {
		for _, c := range open {
			_ = c.Close()
		}
	}()
	for _, p := range set.parts {
		op := wire.OutPart{Size: p.desc.Size}
		if p.desc.Kind == PartFile {
			op.Kind, op.Name = "file", p.desc.Name
		} else {
			op.Kind, op.Name = "fmt", p.desc.MIME
		}
		if p.data != nil {
			op.R = bytesReader(p.data)
		} else {
			rc, oerr := e.openPart(ctx, p)
			if oerr != nil {
				return false, oerr
			}
			open = append(open, rc)
			op.R = rc
		}
		out = append(out, op)
	}
	if err := w.SendStream(id, origin, ts, set.hash, out); err != nil {
		return true, err
	}
	return false, nil
}

// reportTarget records how a send fares towards one device and finishes the
// operation when every target is final.
func (e *Engine) reportTarget(id OperationID, peer DeviceID, st session.TargetState, cause error) {
	e.mu.Lock()
	o := e.ops.byID[id]
	if o == nil {
		e.mu.Unlock()
		return
	}
	var t *TargetStatus
	for i := range o.op.Targets {
		if o.op.Targets[i].PeerID == peer {
			t = &o.op.Targets[i]
		}
	}
	if t == nil || t.State.final() {
		e.mu.Unlock()
		return
	}
	switch st {
	case session.Sending:
		t.State = TargetSending
	case session.Sent:
		t.State = TargetSent
	case session.Superseded:
		t.State = TargetSuperseded
	case session.Failed:
		t.State = TargetFailed
		t.Error = e.sendFailure(cause)
	}
	o.op.Phase, o.op.Outcome = phaseOf(o.op.Targets), outcomeOf(o.op.Targets)
	o.op.UpdatedAtMs = time.Now().UnixMilli()
	var done *contentObj
	if o.op.Phase == PhaseCompleted && o.content != nil {
		done, o.content = o.content, nil
	}
	e.commitObjsLocked([]ObjectRef{{Type: "operation", ID: string(id)}}, EventOperations)
	e.mu.Unlock()
	if done != nil {
		e.cs.unref(done)
	}
}

// sendFailure classifies why a transfer failed, without forwarding text.
func (e *Engine) sendFailure(cause error) *Error {
	var ee *Error
	switch {
	case errors.As(cause, &ee):
		return ee
	case errors.Is(cause, session.ErrSessionClosed):
		if e.runtime == RuntimeSuspended {
			return newErr(CodeSuspended, "the host suspended networking during the transfer")
		}
		return retryable(newErr(CodeUnavailable, "the connection ended before the content was sent"))
	}
	return retryable(newErr(CodeUnavailable, "the transfer failed"))
}

func phaseOf(ts []TargetStatus) Phase {
	allQueued, allFinal := true, true
	for _, t := range ts {
		allQueued = allQueued && t.State == TargetQueued
		allFinal = allFinal && t.State.final()
	}
	switch {
	case allFinal:
		return PhaseCompleted
	case allQueued:
		return PhaseQueued
	}
	return PhaseSending
}

func outcomeOf(ts []TargetStatus) Outcome {
	if phaseOf(ts) != PhaseCompleted {
		return ""
	}
	var sent, sup int
	for _, t := range ts {
		switch t.State {
		case TargetSent:
			sent++
		case TargetSuperseded:
			sup++
		}
	}
	switch {
	case sent == len(ts):
		return OutcomeSuccess
	case sup == len(ts):
		return OutcomeSuperseded
	case sent > 0:
		return OutcomePartial
	}
	return OutcomeFailed
}

func bytesReader(b []byte) io.Reader { return bytes.NewReader(b) }

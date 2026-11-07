package engine

import (
	"bytes"
	"context"
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

func syncAllowsPart(p *partRec, sy backendPolicy) bool {
	switch {
	case p.desc.Kind == PartFile:
		return sy.Files
	case p.desc.MIME == MimePNG:
		return sy.Images
	default:
		return sy.Text
	}
}

// buildSendSet applies the sync filters (0002) to a content.
func buildSendSet(c *contentObj, sy backendPolicy) sendSet {
	set := sendSet{c: c}
	var snap clip.Snapshot
	for _, p := range c.parts {
		if !syncAllowsPart(p, sy) {
			continue
		}
		switch {
		case p.desc.Kind == PartFile:
			snap.Files = append(snap.Files, clip.File{Name: p.desc.Name, Size: p.desc.Size})
		default:
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
	if !validExternalSubmissionID(req.RequestID) {
		return "", newErr(CodeInvalidArgument, "invalid submission RequestID")
	}
	return e.submit(ctx, req)
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
func (e *Engine) sendSet(ctx context.Context, w *wire.Conn, origin, id string, ts int64, set sendSet, policy func() backendPolicy) (broken bool, err error) {
	e.mu.Lock()
	if err := e.needNetworkLocked(); err != nil {
		e.mu.Unlock()
		return false, err
	}
	enabled, lim := e.settings.Value.Sync.Enabled, e.settings.Value.Limits
	e.mu.Unlock()
	if !enabled {
		return false, newErr(CodeSyncDisabled, "sync was paused before the transfer started")
	}
	sy := policy()
	// A queued view must never send a type disabled since it was prepared.
	for _, p := range set.parts {
		if !syncAllowsPart(p, sy) {
			return false, newErr(CodeSyncDisabled, "a content type was disabled before the transfer started")
		}
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

// reportTarget records how a send fares towards one target and finishes the
// operation when every target is final and no backend can still register more.
func (e *Engine) reportTarget(id OperationID, k targetKey, st TargetState, cause error) {
	e.mu.Lock()
	o := e.ops.byID[id]
	if o == nil {
		e.mu.Unlock()
		return
	}
	var t *TargetStatus
	for i := range o.op.Targets {
		if x := &o.op.Targets[i]; x.BackendID == k.backend && x.TargetKind == k.kind && x.TargetID == k.id {
			t = x
		}
	}
	if t == nil || t.State.final() {
		e.mu.Unlock()
		return
	}
	t.State = st
	if st == TargetFailed {
		t.Error = e.sendFailure(cause)
	}
	done := e.refreshOpLocked(o)
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

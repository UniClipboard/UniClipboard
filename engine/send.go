package engine

import (
	"context"
	"errors"
	"time"

	"github.com/uniclipboard/unic/internal/clip"
	"github.com/uniclipboard/unic/internal/directory"
	"github.com/uniclipboard/unic/internal/peerbackend"
	"github.com/uniclipboard/unic/plugin/rpc"
)

const opProgressEventInterval = 200 * time.Millisecond

// sendSet is what will go out: the parts of a content that the sync settings
// allow, fixed at submission.
type sendSet struct {
	c     *contentObj
	parts []*partRec
	hash  string
	total int64
}

func syncAllowsPart(p *partRec, sy BackendSyncPolicy) bool {
	switch {
	case p.desc.Kind == PartFile || p.desc.MIME == directory.MIME:
		return sy.Files
	case p.desc.MIME == MimePNG:
		return sy.Images
	default:
		return sy.Text
	}
}

// buildSendSet applies the sync filters (0002) to a content.
func buildSendSet(c *contentObj, sy BackendSyncPolicy) sendSet {
	set := sendSet{c: c}
	var snap clip.Snapshot
	for _, p := range c.parts {
		if !syncAllowsPart(p, sy) {
			continue
		}
		switch p.desc.Kind {
		case PartFile:
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

// reportTargetReceipt records how a send fares towards one target (including
// its verified receipt stage) and finishes the operation when every target is
// final and no backend can still register more.
func (e *Engine) reportTargetReceipt(id OperationID, k targetKey, st TargetState, stage ReceiptStage, cause error) {
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
	if stage != "" {
		t.ReceiptStage = stage
	}
	if (st == TargetSent || st == TargetDiscarded) && t.Progress != nil {
		t.Progress.BytesSent = t.Progress.TotalBytes
	}
	if st == TargetFailed {
		t.Error = e.sendFailure(cause)
	}
	done := e.refreshOpLocked(o)
	e.mu.Unlock()
	if done != nil {
		e.cs.unref(done)
		e.opFinished(id)
	}
}

func (e *Engine) reportTargetRoute(id OperationID, k targetKey, route TransportRoute) {
	if route == "" {
		return
	}
	e.mu.Lock()
	defer e.mu.Unlock()
	o := e.ops.byID[id]
	if o == nil {
		return
	}
	for i := range o.op.Targets {
		if x := &o.op.Targets[i]; x.BackendID == k.backend && x.TargetKind == k.kind && x.TargetID == k.id {
			if x.TransportRoute == route {
				return
			}
			x.TransportRoute = route
			o.op.UpdatedAtMs = time.Now().UnixMilli()
			e.commitObjsLocked([]ObjectRef{{Type: "operation", ID: string(id)}}, EventOperations)
			return
		}
	}
}

// reportTargetProgress updates in-memory byte/part progress for one in-flight
// target and coalesces EventOperations notifications to avoid event-queue floods.
func (e *Engine) reportTargetProgress(id OperationID, k targetKey, stage ReceiptStage, p TransferProgress) {
	e.mu.Lock()
	defer e.mu.Unlock()
	o := e.ops.byID[id]
	if o == nil {
		return
	}
	var t *TargetStatus
	for i := range o.op.Targets {
		if x := &o.op.Targets[i]; x.BackendID == k.backend && x.TargetKind == k.kind && x.TargetID == k.id {
			t = x
		}
	}
	if t == nil || t.State.final() {
		return
	}
	if p.TotalBytes < 0 || p.BytesSent < 0 || p.BytesSent > p.TotalBytes {
		return
	}
	if t.Progress != nil && p.BytesSent < t.Progress.BytesSent {
		return
	}
	stageChanged := stage != "" && stage != t.ReceiptStage
	if stage != "" {
		t.ReceiptStage = stage
	}
	cp := p
	t.Progress = &cp
	now := time.Now()
	o.op.UpdatedAtMs = now.UnixMilli()
	if stageChanged || o.lastProgressEvent.IsZero() || now.Sub(o.lastProgressEvent) >= opProgressEventInterval {
		o.lastProgressEvent = now
		e.commitObjsLocked([]ObjectRef{{Type: "operation", ID: string(o.op.ID)}}, EventOperations)
	}
}

// sendFailure classifies why a transfer failed, without forwarding text.
func (e *Engine) sendFailure(cause error) *Error {
	var ee *Error
	switch {
	case errors.As(cause, &ee):
		return ee
	case !e.settings.Value.Sync.Enabled:
		return newErr(CodeSyncDisabled, "sync was turned off during the transfer")
	case isRPCCode(cause, string(CodeSourceChanged)):
		return newErr(CodeSourceChanged, "the content changed after it was added")
	case isRPCCode(cause, string(CodeResourceExpired)):
		return newErr(CodeResourceExpired, "the content is no longer available")
	case e.runtime == RuntimeSuspended:
		return newErr(CodeSuspended, "the host suspended networking during the transfer")
	case isRPCCode(cause, string(CodeTimeout)), errors.Is(cause, context.DeadlineExceeded):
		return retryable(newErr(CodeTimeout, "the transfer timed out"))
	case isRPCCode(cause, string(CodeCancelled)), errors.Is(cause, context.Canceled):
		return newErr(CodeCancelled, "the transfer was cancelled")
	case errors.Is(cause, peerbackend.ErrSessionClosed):
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

// isRPCCode reports whether err is a plugin RPC error with the given code.
func isRPCCode(err error, code string) bool {
	var re *rpc.Error
	return errors.As(err, &re) && re.Code == code
}

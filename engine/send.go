package engine

import (
	"bytes"
	"context"
	"errors"
	"io"

	"github.com/uniclipboard/unic/internal/clip"
	"github.com/uniclipboard/unic/internal/peerbackend"
	"github.com/uniclipboard/unic/plugin/rpc"
)

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
	case p.desc.Kind == PartFile:
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
		e.opFinished(id)
	}
}

// sendFailure classifies why a transfer failed, without forwarding text.
func (e *Engine) sendFailure(cause error) *Error {
	var ee *Error
	switch {
	case errors.As(cause, &ee):
		return ee
	case isRPCCode(cause, string(CodeSourceChanged)):
		return newErr(CodeSourceChanged, "the content changed after it was added")
	case isRPCCode(cause, string(CodeResourceExpired)):
		return newErr(CodeResourceExpired, "the content is no longer available")
	case e.runtime == RuntimeSuspended:
		return newErr(CodeSuspended, "the host suspended networking during the transfer")
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

func bytesReader(b []byte) io.Reader { return bytes.NewReader(b) }

// isRPCCode reports whether err is a plugin RPC error with the given code.
func isRPCCode(err error, code string) bool {
	var re *rpc.Error
	return errors.As(err, &re) && re.Code == code
}

package engine

import (
	"context"
	"crypto/sha256"
	"errors"
	"sort"
	"time"
)

const receiveProgressEventMinInterval = 200 * time.Millisecond

// opEntry is one retained operation and the fingerprint of the request that
// made it. Only a hash of sensitive parameters is kept, never the parameters.
type opEntry struct {
	cancel            func() // release the submission context after all targets finish
	discardReceive    func() // cancel an in-flight inbound peer receive
	op                Operation
	hash              [sha256.Size]byte
	content           *contentObj // a send holds its content until every target is final
	open              bool        // backends may still register targets; guarded by Engine.mu
	// history links a send to the record it belongs to, so that the record's
	// sync summary follows the operation. results are the commit decisions.
	history           HistoryID
	results           []BackendSubmissionResult
	manual            bool
	productRecorded   bool
	kind              DiagnosticContentKind
	recorded          bool // the final summary was written
	lastProgressEvent time.Time
}

type opTable struct {
	byID map[OperationID]*opEntry
	reqs map[RequestID]*opEntry
}

func newOpTable() opTable {
	return opTable{byID: map[OperationID]*opEntry{}, reqs: map[RequestID]*opEntry{}}
}

func (t *opTable) drop(o *opEntry) {
	delete(t.byID, o.op.ID)
	if t.reqs[o.op.RequestID] == o {
		delete(t.reqs, o.op.RequestID)
	}
}

// prune removes finished operations older than ttl; they stop being queryable
// and their RequestIDs are forgotten.
func (t *opTable) prune(now time.Time, ttl time.Duration) {
	for _, o := range t.byID {
		if o.op.Done() && now.UnixMilli()-o.op.UpdatedAtMs > ttl.Milliseconds() {
			t.drop(o)
		}
	}
}

// list returns the retained operations, oldest first.
func (t *opTable) list() []Operation {
	out := make([]Operation, 0, len(t.byID))
	for _, o := range t.byID {
		out = append(out, o.op.clone())
	}
	sort.Slice(out, func(i, j int) bool {
		if out[i].CreatedAtMs != out[j].CreatedAtMs {
			return out[i].CreatedAtMs < out[j].CreatedAtMs
		}
		return out[i].ID < out[j].ID
	})
	return out
}

// newOpLocked registers an operation for req, or finds the one already made
// for the same request. e.mu must be held. existing is true when no new work
// should start.
func (e *Engine) newOpLocked(kind OperationKind, req RequestID, hash [sha256.Size]byte, phase Phase) (o *opEntry, existing bool, err *Error) {
	now := time.Now()
	e.ops.prune(now, e.res.OperationTTL)
	e.pruneSubmissionsLocked(now)
	if submission := e.submissions[req]; submission != nil && (kind != OperationSend || submission.hash != hash) {
		return nil, false, newErr(CodeConflict, "this RequestID was used for a different request")
	}
	if old := e.ops.reqs[req]; old != nil {
		if old.op.Kind != kind || old.hash != hash {
			return nil, false, newErr(CodeConflict, "this RequestID was used for a different request")
		}
		return old, true, nil
	}
	if len(e.ops.byID) >= e.res.MaxOperations {
		// Make room by evicting the oldest finished operation; unfinished ones stay.
		var oldest *opEntry
		for _, c := range e.ops.byID {
			if c.op.Done() && (oldest == nil || c.op.UpdatedAtMs < oldest.op.UpdatedAtMs) {
				oldest = c
			}
		}
		if oldest == nil {
			return nil, false, retryable(newErr(CodeResourceExhausted, "too many operations in progress"))
		}
		e.ops.drop(oldest)
	}
	ms := now.UnixMilli()
	o = &opEntry{hash: hash, op: Operation{ID: OperationID(e.newID()), RequestID: req, Kind: kind,
		CreatedAtMs: ms, UpdatedAtMs: ms, Phase: phase}}
	e.ops.byID[o.op.ID] = o
	e.ops.reqs[req] = o
	e.commitObjsLocked([]ObjectRef{{Type: "operation", ID: string(o.op.ID)}}, EventOperations)
	return o, false, nil
}

// updateOp changes an operation and announces it.
func (e *Engine) updateOp(id OperationID, f func(*Operation)) {
	e.mu.Lock()
	defer e.mu.Unlock()
	o := e.ops.byID[id]
	if o == nil {
		return
	}
	f(&o.op)
	o.op.UpdatedAtMs = time.Now().UnixMilli()
	e.commitObjsLocked([]ObjectRef{{Type: "operation", ID: string(id)}}, EventOperations)
}

// startReceiveOp registers an in-memory receive operation for an admitted peer
// transfer. If the operations table is full of unfinished operations, it returns
// an empty ID so the receive still succeeds without blocking sync.
func (e *Engine) startReceiveOp(recv ReceiveOperation, discard func()) OperationID {
	e.mu.Lock()
	defer e.mu.Unlock()
	now := time.Now()
	e.ops.prune(now, e.res.OperationTTL)
	if len(e.ops.byID) >= e.res.MaxOperations {
		var oldest *opEntry
		for _, c := range e.ops.byID {
			if c.op.Done() && (oldest == nil || c.op.UpdatedAtMs < oldest.op.UpdatedAtMs) {
				oldest = c
			}
		}
		if oldest == nil {
			return ""
		}
		e.ops.drop(oldest)
	}
	ms := now.UnixMilli()
	r := recv
	o := &opEntry{
		discardReceive: discard,
		op: Operation{
			ID:          OperationID(e.newID()),
			Kind:        OperationReceive,
			CreatedAtMs: ms,
			UpdatedAtMs: ms,
			Phase:       PhaseReceiving,
			Receive:     &r,
		},
	}
	e.ops.byID[o.op.ID] = o
	e.commitObjsLocked([]ObjectRef{{Type: "operation", ID: string(o.op.ID)}}, EventOperations)
	return o.op.ID
}

// updateReceiveProgress updates received bytes for an active inbound transfer
// and coalesces EventOperations notifications to avoid event buffer pressure.
func (e *Engine) updateReceiveProgress(id OperationID, bytesReceived int64, partIndex int) {
	if id == "" {
		return
	}
	e.mu.Lock()
	defer e.mu.Unlock()
	o := e.ops.byID[id]
	if o == nil || o.op.Receive == nil || o.op.Receive.State != ReceiveReceiving {
		return
	}
	if bytesReceived < o.op.Receive.Progress.BytesReceived {
		return
	}
	if o.op.Receive.Progress.TotalBytes > 0 && bytesReceived > o.op.Receive.Progress.TotalBytes {
		bytesReceived = o.op.Receive.Progress.TotalBytes
	}
	o.op.Receive.Progress.BytesReceived = bytesReceived
	o.op.Receive.Progress.PartIndex = partIndex
	now := time.Now()
	o.op.UpdatedAtMs = now.UnixMilli()
	if o.lastProgressEvent.IsZero() || now.Sub(o.lastProgressEvent) >= receiveProgressEventMinInterval || (o.op.Receive.Progress.TotalBytes > 0 && bytesReceived >= o.op.Receive.Progress.TotalBytes) {
		o.lastProgressEvent = now
		e.commitObjsLocked([]ObjectRef{{Type: "operation", ID: string(id)}}, EventOperations)
	}
}

// finishReceiveOp transitions a receive operation to its terminal state.
func (e *Engine) finishReceiveOp(id OperationID, state ReceiveState, cause error) {
	if id == "" {
		return
	}
	e.mu.Lock()
	defer e.mu.Unlock()
	o := e.ops.byID[id]
	if o == nil || o.op.Receive == nil {
		return
	}
	o.discardReceive = nil
	if o.op.Receive.State == ReceiveDiscarded {
		return
	}
	o.op.Receive.State = state
	o.op.Phase = PhaseCompleted
	if state == ReceiveCommitted && o.op.Receive.Progress.TotalBytes > 0 {
		o.op.Receive.Progress.BytesReceived = o.op.Receive.Progress.TotalBytes
	}
	if state == ReceiveFailed && cause != nil {
		var ee *Error
		if errors.As(cause, &ee) {
			o.op.Error = cloneError(ee)
		} else {
			o.op.Error = newErr(CodeUnavailable, "receive failed")
		}
	}
	o.op.UpdatedAtMs = time.Now().UnixMilli()
	e.commitObjsLocked([]ObjectRef{{Type: "operation", ID: string(id)}}, EventOperations)
}

// DiscardReceive cancels an in-flight inbound peer receive operation. The
// receiver immediately stops writing to disk, deletes unpublished staging files,
// releases reservations, and returns outcome=discarded to the backend so the
// session stays synchronized and notifies the sender with clip_ack(discarded).
func (e *Engine) DiscardReceive(ctx context.Context, id OperationID) error {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return gerr
	}
	defer release()
	e.mu.Lock()
	e.ops.prune(time.Now(), e.res.OperationTTL)
	o := e.ops.byID[id]
	if o == nil {
		foreign := e.foreign(string(id))
		e.mu.Unlock()
		if foreign {
			return newErr(CodeResourceExpired, "the operation belongs to an earlier instance")
		}
		return newErr(CodeNotFound, "no such operation (it may have expired)")
	}
	if o.op.Kind != OperationReceive || o.op.Receive == nil {
		e.mu.Unlock()
		return newErr(CodeInvalidArgument, "operation is not a receive transfer")
	}
	switch o.op.Receive.State {
	case ReceiveDiscarded:
		e.mu.Unlock()
		return nil
	case ReceiveCommitted, ReceiveFailed:
		e.mu.Unlock()
		return newErr(CodeConflict, "receive operation already finished")
	}
	o.op.Receive.State = ReceiveDiscarded
	o.op.Phase = PhaseCompleted
	o.op.UpdatedAtMs = time.Now().UnixMilli()
	fn := o.discardReceive
	o.discardReceive = nil
	e.commitObjsLocked([]ObjectRef{{Type: "operation", ID: string(id)}}, EventOperations)
	e.mu.Unlock()
	if fn != nil {
		fn()
	}
	return nil
}

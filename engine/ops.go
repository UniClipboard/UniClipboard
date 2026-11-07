package engine

import (
	"crypto/sha256"
	"sort"
	"time"
)

// opEntry is one retained operation and the fingerprint of the request that
// made it. Only a hash of sensitive parameters is kept, never the parameters.
type opEntry struct {
	op      Operation
	hash    [sha256.Size]byte
	content *contentObj // a send holds its content until every target is final
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

package engine

import (
	"context"
	"crypto/sha256"
	"time"
)

// The private result cache precedes the public CommitContent API/durable sink.
// It owns metadata only; neither completed entries nor waiters retain content.
type submissionEntry struct {
	hash     [sha256.Size]byte
	done     chan struct{}
	id       OperationID
	err      *Error
	finished time.Time // zero while processing; guarded by Engine.mu
}

func cloneError(err *Error) *Error {
	if err == nil {
		return nil
	}
	copy := *err
	return &copy
}

func (e *Engine) pruneSubmissionsLocked(now time.Time) {
	for id, entry := range e.submissions {
		if !entry.finished.IsZero() && now.Sub(entry.finished) >= e.res.SubmissionTTL {
			delete(e.submissions, id)
		}
	}
}

// startSubmissionLocked takes an independent reference before launching work.
// e.mu guards admission against Stop and WaitGroup.Wait. Callers never wait for
// callbacks while holding this lock (or the observation/application mutex).
func (e *Engine) startSubmissionLocked(req SubmitRequest) (*submissionEntry, *Error) {
	if req.RequestID == "" {
		return nil, newErr(CodeInvalidArgument, "RequestID is required")
	}
	hash := sha256.Sum256([]byte("submit\x00" + string(req.LeaseID) + "\x00" + string(req.Origin)))
	now := time.Now()
	e.pruneSubmissionsLocked(now)
	if old := e.submissions[req.RequestID]; old != nil {
		if old.hash != hash {
			return nil, newErr(CodeConflict, "this RequestID was used for a different request")
		}
		return old, nil
	}
	e.ops.prune(now, e.res.OperationTTL)
	if old := e.ops.reqs[req.RequestID]; old != nil {
		if old.op.Kind != OperationSend || old.hash != hash {
			return nil, newErr(CodeConflict, "this RequestID was used for a different request")
		}
		entry := &submissionEntry{hash: hash, id: old.op.ID, done: make(chan struct{}), finished: now}
		close(entry.done)
		return entry, nil
	}
	if err := e.needNetworkLocked(); err != nil {
		return nil, err
	}
	// Validate before eviction so rejected input cannot displace a cached result.
	e.cs.mu.Lock()
	l := e.cs.leases[req.LeaseID]
	if l == nil {
		gone := e.cs.gone.has(string(req.LeaseID))
		e.cs.mu.Unlock()
		if e.foreign(string(req.LeaseID)) || gone {
			return nil, newErr(CodeResourceExpired, "the lease is no longer valid")
		}
		return nil, newErr(CodeNotFound, "no such lease")
	}
	c := l.c
	c.refs++
	e.cs.mu.Unlock()
	if len(e.submissions) >= e.res.MaxSubmissions {
		var oldest RequestID
		for id, entry := range e.submissions {
			if !entry.finished.IsZero() && (oldest == "" || entry.finished.Before(e.submissions[oldest].finished)) {
				oldest = id
			}
		}
		if oldest == "" {
			e.cs.unref(c)
			return nil, newErr(CodeResourceExhausted, "too many submissions in progress")
		}
		delete(e.submissions, oldest)
	}
	entry := &submissionEntry{hash: hash, done: make(chan struct{})}
	if e.submissions == nil {
		e.submissions = make(map[RequestID]*submissionEntry)
	}
	e.submissions[req.RequestID] = entry
	workCtx := e.runCtx
	e.opWG.Add(1)
	go e.runSubmission(workCtx, req, entry, c)
	return entry, nil
}

func (e *Engine) runSubmission(ctx context.Context, req SubmitRequest, entry *submissionEntry, c *contentObj) {
	defer e.opWG.Done()
	origin := pipelineOrigin{kind: "explicit"}
	if req.Origin == OriginClipboard {
		origin.kind = "local_system"
	}
	processed, failure := e.processContent(ctx, pipelineCaptured, c, origin)
	var id OperationID
	if failure == nil {
		var err error
		id, err = e.dispatchProcessed(ctx, req, entry.hash, processed)
		if err != nil {
			failure = asError(err)
		}
		e.cs.unref(processed.c)
	}
	e.mu.Lock()
	entry.id, entry.err, entry.finished = id, cloneError(failure), time.Now()
	close(entry.done)
	e.mu.Unlock()
}

func waitSubmission(ctx context.Context, entry *submissionEntry) (OperationID, error) {
	select {
	case <-entry.done:
		if entry.err != nil {
			return entry.id, cloneError(entry.err)
		}
		return entry.id, nil
	case <-ctx.Done():
		return "", ctxErr(ctx.Err())
	}
}

func (e *Engine) submit(ctx context.Context, req SubmitRequest) (OperationID, error) {
	e.mu.Lock()
	entry, err := e.startSubmissionLocked(req)
	e.mu.Unlock()
	if err != nil {
		return "", err
	}
	return waitSubmission(ctx, entry)
}

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
	item     ItemID
	history  HistoryID
	id       OperationID
	disp     SyncDisposition // set at commit; empty while processing or failed
	syncErr  *Error          // only disp == SyncFailed
	err      *Error          // terminal failure before commit
	finished time.Time       // zero while processing; guarded by Engine.mu
}

// legacyErr maps the committed result back to the old SubmitContent contract:
// a commit that happened but sent nothing is still reported as an error there.
func (s *submissionEntry) legacyErr() *Error {
	switch {
	case s.err != nil:
		return cloneError(s.err)
	case s.disp == SyncQueued || s.disp == "":
		return nil
	case s.disp == SyncNoTargets:
		return newErr(CodeNoTargets, "no paired device is connected")
	case s.disp == SyncFailed:
		return cloneError(s.syncErr)
	}
	return newErr(CodeSyncDisabled, "sync is disabled for this content")
}

// result is a content-free snapshot. e.mu must be held unless entry.done is closed.
func (s *submissionEntry) result(id RequestID) SubmissionResult {
	r := SubmissionResult{RequestID: id, Phase: SubmissionProcessing, ItemID: s.item}
	switch {
	case s.finished.IsZero():
	case s.err != nil:
		r.Phase, r.Error = SubmissionFailed, cloneError(s.err)
	default:
		r.Phase, r.HistoryID, r.SyncDisposition, r.OperationID, r.SyncError = SubmissionCommitted, s.history, s.disp, s.id, cloneError(s.syncErr)
	}
	return r
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
		entry := &submissionEntry{hash: hash, id: old.op.ID, disp: SyncQueued, done: make(chan struct{}), finished: now}
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
	entry := &submissionEntry{hash: hash, item: ItemID(e.newID()), done: make(chan struct{})}
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
	var disp SyncDisposition
	var syncErr *Error
	var historyID HistoryID
	if failure == nil {
		if processed.policy.Persist {
			// A record that cannot be stored means the commit failed: nothing is sent.
			historyID, failure = e.persistHistory(ctx, processed)
		}
		if failure == nil {
			id, disp, syncErr, failure = e.dispatchProcessed(ctx, req, entry.hash, processed)
		}
		e.cs.unref(processed.c)
	}
	e.mu.Lock()
	entry.history = historyID
	entry.id, entry.disp, entry.syncErr, entry.err, entry.finished = id, disp, syncErr, cloneError(failure), time.Now()
	close(entry.done)
	e.mu.Unlock()
}

func waitSubmission(ctx context.Context, entry *submissionEntry) (OperationID, error) {
	select {
	case <-entry.done:
		if err := entry.legacyErr(); err != nil {
			return entry.id, err
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

// CommitContent processes leased content and reports the commit and the sync
// decision separately. Without devices, backends or sync, the content is
// still processed and committed; only SyncDisposition says nothing was sent.
// A caller timeout says nothing about the work: retry with the same RequestID.
func (e *Engine) CommitContent(ctx context.Context, req CommitRequest) (SubmissionResult, error) {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return SubmissionResult{}, gerr
	}
	defer release()
	if !validExternalSubmissionID(req.RequestID) {
		return SubmissionResult{}, newErr(CodeInvalidArgument, "invalid submission RequestID")
	}
	if req.WritebackGuard != nil {
		return SubmissionResult{}, newErr(CodeUnsupported, "automatic writeback is not implemented")
	}
	if req.BackendIDs != nil {
		return SubmissionResult{}, newErr(CodeNotFound, "no such sync backend")
	}
	e.mu.Lock()
	entry, err := e.startSubmissionLocked(SubmitRequest{RequestID: req.RequestID, LeaseID: req.LeaseID, Origin: OriginExplicit})
	e.mu.Unlock()
	if err != nil {
		return SubmissionResult{}, err
	}
	select {
	case <-entry.done:
		r := entry.result(req.RequestID)
		if r.Error != nil {
			return r, cloneError(r.Error)
		}
		return r, nil
	case <-ctx.Done():
		return SubmissionResult{}, ctxErr(ctx.Err())
	}
}

// GetSubmission returns the retained result of an accepted request. Unknown
// or evicted requests are not_found; it never starts or retries work.
func (e *Engine) GetSubmission(ctx context.Context, id RequestID) (SubmissionResult, error) {
	if err := ctx.Err(); err != nil {
		return SubmissionResult{}, ctxErr(err)
	}
	if !validExternalSubmissionID(id) {
		return SubmissionResult{}, newErr(CodeInvalidArgument, "invalid submission RequestID")
	}
	e.mu.Lock()
	defer e.mu.Unlock()
	if e.closing {
		return SubmissionResult{}, errClosed
	}
	e.pruneSubmissionsLocked(time.Now())
	entry := e.submissions[id]
	if entry == nil {
		return SubmissionResult{}, newErr(CodeNotFound, "no such submission (it may have expired)")
	}
	return entry.result(id), nil
}

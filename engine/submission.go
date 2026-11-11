package engine

import (
	"context"
	"crypto/sha256"
	"sort"
	"time"

	"github.com/uniclipboard/unic/plugin"
)

// The private result cache precedes the public CommitContent API/durable sink.
// It owns metadata only; neither completed entries nor waiters retain content.
type submissionEntry struct {
	started  time.Time
	hash     [sha256.Size]byte
	done     chan struct{}
	item     ItemID
	history  HistoryID
	id       OperationID
	disp     SyncDisposition // set at commit; empty while processing or failed
	results  []BackendSubmissionResult
	syncErr  *Error    // only disp == SyncFailed
	err      *Error    // terminal failure before commit
	finished time.Time // zero while processing; guarded by Engine.mu
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
		r.BackendResults = cloneResults(s.results)
	}
	return r
}

func deviceKey(ids []DeviceID) string {
	if ids == nil {
		return ""
	}
	s := "\x00devices"
	for _, id := range ids {
		s += "\x00" + string(id)
	}
	return s
}

func backendKey(ids []PluginID) string {
	if ids == nil {
		return ""
	}
	s := "\x00backends"
	for _, id := range ids {
		s += "\x00" + string(id)
	}
	return s
}

func cloneResults(rs []BackendSubmissionResult) []BackendSubmissionResult {
	if rs == nil {
		return nil
	}
	out := make([]BackendSubmissionResult, len(rs))
	for i, r := range rs {
		r.Error = cloneError(r.Error)
		out[i] = r
	}
	return out
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
	key := "submit\x00" + string(req.LeaseID) + "\x00" + string(req.Origin) + backendKey(req.BackendIDs) + deviceKey(req.DeviceIDs)
	if req.action != nil {
		key = "action\x00" + req.action.act.key() + "\x00" + string(req.action.source)
	}
	hash := sha256.Sum256([]byte(key))
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
	if err := e.needRunningLocked(); err != nil {
		return nil, err
	}
	// Validate before eviction so rejected input cannot displace a cached result.
	var c *contentObj // an action produces its content inside the submission
	if req.action == nil {
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
		c = l.c
		c.refs++
		e.cs.mu.Unlock()
	}
	if len(e.submissions) >= e.res.MaxSubmissions {
		var oldest RequestID
		for id, entry := range e.submissions {
			if !entry.finished.IsZero() && (oldest == "" || entry.finished.Before(e.submissions[oldest].finished)) {
				oldest = id
			}
		}
		if oldest == "" {
			if c != nil {
				e.cs.unref(c)
			}
			return nil, newErr(CodeResourceExhausted, "too many submissions in progress")
		}
		delete(e.submissions, oldest)
	}
	entry := &submissionEntry{started: now, hash: hash, item: ItemID(e.newID()), done: make(chan struct{})}
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
	commitKind := diagnosticKind(c)
	origin := pipelineOrigin{kind: "explicit"}
	if req.Origin == OriginClipboard {
		origin.kind = "local_system"
	}
	var failure *Error
	if req.action != nil {
		origin = pipelineOrigin{kind: "action", pluginID: string(req.action.act.plugin.id)}
		c, failure = req.action.produce(ctx, e)
	}
	if c != nil {
		commitKind = diagnosticKind(c)
	}
	var processed *processedContent
	if failure == nil {
		processed, failure = e.processContent(ctx, pipelineCaptured, c, origin)
		if failure == nil && req.action != nil {
			processed.policy.Persist = true // the host applies the result through its history item
		}
	}
	var id OperationID
	var disp SyncDisposition
	var results []BackendSubmissionResult
	var syncErr *Error
	var historyID HistoryID
	gateHeld := false
	if failure == nil {
		e.commitGate.RLock()
		gateHeld = true
		if err := ctx.Err(); err != nil {
			failure = ctxErr(err) // stopped or suspended after processing: no late commit
			e.cs.unref(processed.c)
		}
	}
	if failure == nil {
		if processed.policy.Persist {
			// A record that cannot be stored means the commit failed: nothing is sent.
			historyID, failure = e.persistHistory(ctx, processed, nil)
		}
		if failure == nil {
			id, disp, results, syncErr, failure = e.dispatchProcessed(ctx, req, entry.hash, processed)
		}
		e.cs.unref(processed.c)
	}
	if historyID != "" && failure == nil {
		e.linkHistory(historyID, id, results)
		e.notifyObservers(plugin.EventCommitted, string(historyID))
	}
	e.mu.Lock()
	entry.history = historyID
	entry.id, entry.disp, entry.results, entry.syncErr, entry.err, entry.finished = id, disp, results, syncErr, cloneError(failure), time.Now()
	close(entry.done)
	e.mu.Unlock()
	if gateHeld {
		e.commitGate.RUnlock()
	}
	outcome, code := diagnosticOutcome(failure)
	if req.action != nil {
		category := "external"
		if req.action.act.plugin.id == "builtin.text" {
			category = "builtin"
		}
		e.productEvent(ctx, ProductEvent{Name: "action_completed", Result: string(outcome), Code: code, ContentKind: commitKind, ActionCategory: category})
	} else if req.Origin == OriginExplicit && id == "" {
		result := string(outcome)
		if failure == nil {
			result = "skipped"
			if disp == SyncFailed {
				result = "failed"
				if syncErr != nil {
					code = syncErr.Code
				}
			}
		}
		e.productEvent(ctx, ProductEvent{Name: "manual_send_completed", Result: result, Code: code, ContentKind: commitKind})
	}
	e.diagnostic(ctx, DiagnosticRecord{Stage: "commit", Result: outcome, Backend: "none", ContentKind: commitKind, Code: code, Duration: time.Since(entry.started)})
	if e.log != nil {
		if failure == nil {
			e.log.DebugContext(ctx, "pipeline.completed", "stage", "commit", "result", "succeeded")
		} else {
			e.log.WarnContext(ctx, "pipeline.failed", "stage", "commit", "code", string(code))
		}
	}
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
	ids, ierr := e.normalizeBackendIDs(req.BackendIDs)
	if ierr != nil {
		return SubmissionResult{}, ierr
	}
	devs, derr := e.normalizeDeviceIDs(req.DeviceIDs)
	if derr != nil {
		return SubmissionResult{}, derr
	}
	e.mu.Lock()
	entry, err := e.startSubmissionLocked(SubmitRequest{RequestID: req.RequestID, LeaseID: req.LeaseID, Origin: OriginExplicit, BackendIDs: ids, DeviceIDs: devs})
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

// normalizeDeviceIDs validates an explicit device selection: nil means every
// device, an empty list is an error, unknown, revoked or repeated IDs are errors.
func (e *Engine) normalizeDeviceIDs(ids []DeviceID) ([]DeviceID, *Error) {
	if ids == nil {
		return nil, nil
	}
	if len(ids) == 0 {
		return nil, newErr(CodeInvalidArgument, "DeviceIDs must be omitted or non-empty")
	}
	out := append([]DeviceID(nil), ids...)
	sort.Slice(out, func(i, j int) bool { return out[i] < out[j] })
	for i, id := range out {
		if i > 0 && out[i-1] == id {
			return nil, newErr(CodeInvalidArgument, "DeviceIDs has a repeated ID")
		}
		if _, err := e.loadDevice(string(id)); err != nil {
			return nil, newErr(CodeInvalidArgument, "unknown device")
		}
	}
	return out, nil
}

// normalizeBackendIDs validates an explicit backend selection: nil means all
// backends, an empty list is an error, unknown or repeated IDs are errors.
func (e *Engine) normalizeBackendIDs(ids []PluginID) ([]PluginID, *Error) {
	if ids == nil {
		return nil, nil
	}
	if len(ids) == 0 {
		return nil, newErr(CodeInvalidArgument, "BackendIDs must be omitted or non-empty")
	}
	out := append([]PluginID(nil), ids...)
	sort.Slice(out, func(i, j int) bool { return out[i] < out[j] })
	for i, id := range out {
		if i > 0 && out[i-1] == id {
			return nil, newErr(CodeInvalidArgument, "BackendIDs has a repeated ID")
		}
		known := false
		for _, b := range e.syncBackends {
			known = known || b.ID() == id
		}
		if !known {
			return nil, newErr(CodeInvalidArgument, "unknown sync backend")
		}
	}
	return out, nil
}

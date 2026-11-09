package engine

import (
	"context"
	"crypto/sha256"
)

// RestoreSyncResult separates a successful local restore from network work.
type RestoreSyncResult struct {
	LocalApplied    bool            `json:"local_applied"`
	SyncDisposition SyncDisposition `json:"sync_disposition"`
	OperationID     OperationID     `json:"operation_id,omitempty"`
	SyncError       *Error          `json:"sync_error,omitempty"`
}

// SyncRestoredHistory sends the content of the last successful history apply.
// The apply token authorizes one dispatch, without a new commit or processors.
func (e *Engine) SyncRestoredHistory(ctx context.Context, token ApplyToken, lease LeaseID) (RestoreSyncResult, error) {
	release, err := e.enter(ctx)
	if err != nil {
		return RestoreSyncResult{}, err
	}
	defer release()
	out := RestoreSyncResult{LocalApplied: true, SyncDisposition: SyncNotRequested}
	e.commitGate.RLock()
	defer e.commitGate.RUnlock()
	e.mu.Lock()
	done := e.lastApply
	if done.token != token || done.history == "" || done.resent {
		e.mu.Unlock()
		return out, newErr(CodeConflict, "no successful unsent history restore for this token")
	}
	e.lastApply.resent = true
	on := e.settings.Value.Sync.OnRestore
	e.mu.Unlock()
	if !on {
		return out, nil
	}
	it, serr := e.st.GetHistory(string(done.history), historyNow().UnixMilli(), false)
	if serr != nil {
		out.SyncDisposition, out.SyncError = SyncFailed, historyStoreErr(serr)
		return out, nil
	}
	c, cerr := e.lookupContent(lease, true)
	if cerr != nil {
		out.SyncDisposition, out.SyncError = SyncFailed, cerr
		return out, nil
	}
	defer e.cs.unref(c)
	req := SubmitRequest{RequestID: RequestID("restore-" + string(token)), Origin: OriginExplicit}
	pc := &processedContent{c: c, policy: pipelinePolicy{Sync: !it.SyncBlocked}}
	id, disp, results, syncErr, fail := e.dispatchProcessed(ctx, req, sha256.Sum256([]byte(req.RequestID)), pc)
	out.OperationID, out.SyncDisposition, out.SyncError = id, disp, syncErr
	if fail != nil {
		out.SyncDisposition, out.SyncError = SyncFailed, fail
	}
	if fail == nil {
		e.linkHistory(done.history, id, results)
	}
	return out, nil
}

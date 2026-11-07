package engine

import (
	"time"

	"github.com/uniclipboard/unic/internal/store"
)

// A record's sync summary (history-contract §2): not_requested until a send is
// attempted, pending while targets are in flight, then sent / partial /
// failed. A restart turns pending into interrupted; nothing resumes.

type backendSummary struct {
	state HistorySyncState
	disp  BackendDisposition
	code  Code
}

// summarize derives the per-backend and overall state from the commit
// decisions and the current target states.
func summarize(results []BackendSubmissionResult, targets []TargetStatus) (HistorySyncState, map[PluginID]backendSummary) {
	out := make(map[PluginID]backendSummary, len(results))
	anyPending, anySuccess, anyFailure, anySkipped := false, false, false, false
	for _, r := range results {
		sum := backendSummary{state: HistorySyncNotRequested, disp: r.Disposition}
		switch r.Disposition {
		case BackendFailed:
			sum.state = HistorySyncFailed
			if r.Error != nil {
				sum.code = r.Error.Code
			}
		case BackendPolicyFiltered, BackendAutoFileTooLarge:
			anySkipped = true
		case BackendQueued:
			sent, other, open := 0, 0, 0
			for _, t := range targets {
				if t.BackendID != r.BackendID {
					continue
				}
				switch {
				case !t.State.final():
					open++
				case t.State == TargetSent:
					sent++
				default:
					other++
					if sum.code == "" && t.Error != nil {
						sum.code = t.Error.Code
					}
				}
			}
			switch {
			case open > 0:
				sum.state = HistorySyncPending
			case other == 0 && sent > 0:
				sum.state = HistorySyncSent
			case sent > 0:
				sum.state = HistorySyncPartial
			default:
				sum.state = HistorySyncFailed
			}
		}
		switch sum.state {
		case HistorySyncPending:
			anyPending = true
		case HistorySyncSent:
			anySuccess = true
		case HistorySyncPartial:
			anySuccess, anyFailure = true, true
		case HistorySyncFailed:
			anyFailure = true
		}
		out[r.BackendID] = sum
	}
	switch {
	case anyPending:
		return HistorySyncPending, out
	case anySuccess && (anyFailure || anySkipped):
		return HistorySyncPartial, out
	case anySuccess:
		return HistorySyncSent, out
	case anyFailure:
		return HistorySyncFailed, out
	}
	return HistorySyncNotRequested, out
}

// recordSync writes the summary of a record. Nothing is written when no
// backend was asked to do anything (the default already says not_requested),
// so ordinary local copies do not churn revisions or events.
func (e *Engine) recordSync(id HistoryID, results []BackendSubmissionResult, targets []TargetStatus) {
	touched := false
	for _, r := range results {
		touched = touched || r.Disposition == BackendQueued || r.Disposition == BackendFailed
	}
	if !touched {
		return
	}
	state, per := summarize(results, targets)
	backends := make(map[string]store.BackendSync, len(per))
	for b, s := range per {
		backends[string(b)] = store.BackendSync{State: string(s.state), Disposition: string(s.disp), ErrorCode: string(s.code)}
	}
	e.syncMu.Lock() // the later writer must also hold the newer picture
	defer e.syncMu.Unlock()
	e.hist.mu.Lock()
	_, err := e.st.SetSyncState(string(id), string(state), backends, time.Now().UnixMilli())
	e.hist.mu.Unlock()
	if err == nil {
		e.historyChanged(string(id))
	}
}

// linkHistory ties a just-dispatched operation to its record and writes the
// first summary (which is already final if every target ended meanwhile).
func (e *Engine) linkHistory(hid HistoryID, opID OperationID, results []BackendSubmissionResult) {
	var targets []TargetStatus
	e.mu.Lock()
	o := e.ops.byID[opID]
	if o != nil {
		o.history, o.results = hid, results
		targets = append(targets, o.op.Targets...)
		o.recorded = o.op.Phase == PhaseCompleted
	}
	e.mu.Unlock()
	e.recordSync(hid, results, targets)
}

// opFinished writes the final summary once, when an operation completed.
func (e *Engine) opFinished(id OperationID) {
	e.mu.Lock()
	o := e.ops.byID[id]
	if o == nil || o.history == "" || o.recorded || o.op.Phase != PhaseCompleted {
		e.mu.Unlock()
		return
	}
	o.recorded = true
	hid, results, targets := o.history, o.results, append([]TargetStatus(nil), o.op.Targets...)
	e.mu.Unlock()
	e.recordSync(hid, results, targets)
}

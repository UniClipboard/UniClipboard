package host

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"sync"
	"time"

	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
)

type submissionRecord struct {
	cancel context.CancelFunc
	hash   [32]byte
	done   chan struct{}
	at     time.Time
	out    engine.SubmissionResult
	err    error
}
type submissionState struct {
	mu      sync.Mutex
	wg      sync.WaitGroup
	closed  bool
	entries map[string]*submissionRecord
}

func (s *submissionState) close() { s.mu.Lock(); s.closed = true; s.mu.Unlock(); s.wg.Wait() }
func (n *Node) Submit(ctx context.Context, v api.SubmitInput) (engine.SubmissionResult, error) {
	if v.RequestID == "" || len(v.RequestID) > 128 {
		return engine.SubmissionResult{}, &api.Error{Status: 400, Reason: "invalid_argument", Message: "request_id is required (at most 128 bytes)"}
	}
	digestInput := v
	digestInput.Async = false
	encoded, _ := json.Marshal(digestInput)
	hash := sha256.Sum256(encoded)
	s := &n.submissions
	s.mu.Lock()
	if s.closed {
		s.mu.Unlock()
		return engine.SubmissionResult{}, apiErr(adapterError(engine.CodeClosed), false)
	}
	if s.entries == nil {
		s.entries = map[string]*submissionRecord{}
	}
	for id, r := range s.entries {
		if !r.at.IsZero() && time.Since(r.at) > 30*time.Minute {
			if r.cancel != nil {
				r.cancel()
			}
			delete(s.entries, id)
		}
	}
	r := s.entries[v.RequestID]
	if r != nil && r.hash != hash {
		s.mu.Unlock()
		return engine.SubmissionResult{}, apiErr(adapterError(engine.CodeConflict), false)
	}
	if r == nil {
		if len(s.entries) >= 1024 {
			s.mu.Unlock()
			return engine.SubmissionResult{}, apiErr(adapterError(engine.CodeResourceExhausted), false)
		}
		workCtx, cancel := context.WithCancel(n.runCtx)
		r = &submissionRecord{cancel: cancel, hash: hash, done: make(chan struct{})}
		s.entries[v.RequestID] = r
		s.wg.Add(1)
		go func() {
			defer s.wg.Done()
			out, err := n.commitExplicit(workCtx, v)
			if err != nil && out.RequestID == "" {
				out = engine.SubmissionResult{RequestID: engine.RequestID(v.RequestID), Phase: engine.SubmissionFailed, Error: &engine.Error{Code: engine.Code(api.ReasonOf(err)), Message: "explicit submission failed"}}
			}
			s.mu.Lock()
			r.out, r.err, r.at = out, err, time.Now()
			close(r.done)
			s.mu.Unlock()
		}()
	}
	s.mu.Unlock()
	if v.Async {
		select {
		case <-r.done:
			return r.out, r.err
		default:
			return engine.SubmissionResult{RequestID: engine.RequestID(v.RequestID), Phase: engine.SubmissionProcessing}, nil
		}
	}
	select {
	case <-r.done:
		return r.out, r.err
	case <-ctx.Done():
		return engine.SubmissionResult{}, apiErr(adapterError(engine.CodeTimeout), false)
	}
}
func deviceIDs(ids []string) []engine.DeviceID {
	if ids == nil {
		return nil
	}
	out := make([]engine.DeviceID, len(ids))
	for i, id := range ids {
		out[i] = engine.DeviceID(id)
	}
	return out
}

func (n *Node) commitExplicit(ctx context.Context, v api.SubmitInput) (engine.SubmissionResult, error) {
	var h engine.ContentHandle
	var e error
	if v.HistoryID != "" {
		if len(v.Snapshot.Parts)+len(v.Snapshot.Files) > 0 {
			return engine.SubmissionResult{}, &api.Error{Status: 400, Reason: "invalid_argument", Message: "choose history or snapshot"}
		}
		h, e = n.e.RetainHistoryForSend(ctx, engine.HistoryID(v.HistoryID))
	} else {
		input, err := contentInput(v.Snapshot)
		if err != nil {
			return engine.SubmissionResult{}, &api.Error{Status: 400, Reason: "invalid_argument", Message: "invalid content"}
		}
		h, e = n.e.ImportContent(ctx, input)
	}
	if e != nil {
		return engine.SubmissionResult{}, apiErr(e, false)
	}
	defer func() { _ = n.e.ReleaseContent(context.Background(), h.LeaseID) }()
	out, e := n.e.CommitContent(ctx, engine.CommitRequest{CancelOnContext: true, RequestID: engine.RequestID(v.RequestID), LeaseID: h.LeaseID, DeviceIDs: deviceIDs(v.DeviceIDs)})
	if e == nil && out.SyncDisposition == engine.SyncQueued {
		info := "text"
		for _, p := range v.Snapshot.Parts {
			if p.Format == "image/png" {
				info = "image"
			}
		}
		if len(v.Snapshot.Files) > 0 {
			info = fmt.Sprintf("%d files", len(v.Snapshot.Files))
		}
		n.hub.emit(api.Event{Kind: "sent", Info: info})
	}
	return out, apiErr0(e)
}

func (n *Node) CancelSubmission(ctx context.Context, id string) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	n.submissions.mu.Lock()
	r := n.submissions.entries[id]
	if r != nil && r.cancel != nil {
		r.cancel()
	}
	n.submissions.mu.Unlock()
	err := n.e.CancelSubmission(ctx, engine.RequestID(id))
	if r != nil {
		return nil
	}
	return apiErr0(err)
}

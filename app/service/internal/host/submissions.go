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
	hash [32]byte
	done chan struct{}
	at   time.Time
	out  engine.SubmissionResult
	err  error
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
	encoded, _ := json.Marshal(v.Snapshot)
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
		r = &submissionRecord{hash: hash, done: make(chan struct{})}
		s.entries[v.RequestID] = r
		s.wg.Add(1)
		go func() {
			defer s.wg.Done()
			out, err := n.commitExplicit(n.runCtx, v)
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
	select {
	case <-r.done:
		return r.out, r.err
	case <-ctx.Done():
		return engine.SubmissionResult{}, apiErr(adapterError(engine.CodeTimeout), false)
	}
}
func (n *Node) commitExplicit(ctx context.Context, v api.SubmitInput) (engine.SubmissionResult, error) {
	input, e := contentInput(v.Snapshot)
	if e != nil {
		return engine.SubmissionResult{}, &api.Error{Status: 400, Reason: "invalid_argument", Message: "invalid content"}
	}
	h, e := n.e.ImportContent(ctx, input)
	if e != nil {
		return engine.SubmissionResult{}, apiErr(e, false)
	}
	defer func() { _ = n.e.ReleaseContent(context.Background(), h.LeaseID) }()
	out, e := n.e.CommitContent(ctx, engine.CommitRequest{RequestID: engine.RequestID(v.RequestID), LeaseID: h.LeaseID})
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

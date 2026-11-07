package engine

import (
	"context"
	"sync/atomic"
	"testing"
	"time"
)

func TestSubmissionRetainsFailuresWithoutContent(t *testing.T) {
	n := newContentNode(t, nil)
	var calls atomic.Int32
	n.e.processors = []*pipelineProcessor{newPipelineProcessor("count", []pipelineEvent{pipelineCaptured}, false, func(context.Context, *processingContext) error {
		calls.Add(1)
		return nil
	})}
	start(t, n)
	h := importText(t, n.e, "input")
	req := SubmitRequest{RequestID: "retained-failure", LeaseID: h.LeaseID, Origin: OriginExplicit}
	if _, err := n.e.SubmitContent(bg, req); code(err) != CodeNoTargets {
		t.Fatal(err)
	}
	_ = n.e.ReleaseContent(bg, h.LeaseID)
	updateSettings(t, n.e, func(s *Settings) { s.Sync.Enabled = false })
	if _, err := n.e.SubmitContent(bg, req); code(err) != CodeNoTargets {
		t.Fatalf("retry changed its result: %v", err)
	}
	if calls.Load() != 1 || stored(n.e) != 0 {
		t.Fatal("failure retry reran processing or cache retained content")
	}
	req.LeaseID = "different"
	if _, err := n.e.SubmitContent(bg, req); code(err) != CodeConflict {
		t.Fatal(err)
	}
}

func TestSubmissionCallerCancellationAndCapacity(t *testing.T) {
	n := newContentNode(t, func(o *Options) { o.Resources.MaxSubmissions = 1 })
	entered, proceed := make(chan struct{}), make(chan struct{})
	var calls atomic.Int32
	n.e.processors = []*pipelineProcessor{newPipelineProcessor("wait", []pipelineEvent{pipelineCaptured}, false, func(ctx context.Context, _ *processingContext) error {
		if calls.Add(1) == 1 {
			close(entered)
			select {
			case <-proceed:
			case <-ctx.Done():
				return ctx.Err()
			}
		}
		return nil
	})}
	start(t, n)
	h := importText(t, n.e, "input")
	req := SubmitRequest{RequestID: "cancel-wait", LeaseID: h.LeaseID, Origin: OriginExplicit}
	ctx, cancel := context.WithCancel(bg)
	done := make(chan error, 1)
	go func() { _, err := n.e.SubmitContent(ctx, req); done <- err }()
	<-entered
	cancel()
	if err := <-done; code(err) != CodeCancelled {
		t.Fatal(err)
	}
	other := req
	other.RequestID = "next"
	if _, err := n.e.SubmitContent(bg, other); code(err) != CodeResourceExhausted {
		t.Fatalf("processing entry evicted: %v", err)
	}
	_ = n.e.ReleaseContent(bg, h.LeaseID)
	close(proceed)
	if _, err := n.e.SubmitContent(bg, req); code(err) != CodeNoTargets {
		t.Fatalf("cancelled caller cancelled admitted work: %v", err)
	}
	if calls.Load() != 1 || stored(n.e) != 0 {
		t.Fatal("work repeated or leaked")
	}
	// A terminal entry can be evicted; an invalid request cannot evict it.
	if _, err := n.e.SubmitContent(bg, other); code(err) != CodeResourceExpired {
		t.Fatal(err)
	}
	if _, err := n.e.SubmitContent(bg, req); code(err) != CodeNoTargets {
		t.Fatal(err)
	}
	h2 := importText(t, n.e, "next")
	other.LeaseID = h2.LeaseID
	if _, err := n.e.SubmitContent(bg, other); code(err) != CodeNoTargets {
		t.Fatal(err)
	}
	_ = n.e.ReleaseContent(bg, h2.LeaseID)
	if _, err := n.e.SubmitContent(bg, req); code(err) != CodeResourceExpired {
		t.Fatalf("old terminal entry was not evicted: %v", err)
	}
}

func TestObservationAdmitsNewEpochDuringProcessing(t *testing.T) {
	n := newContentNode(t, withHost)
	entered, proceed := make(chan struct{}), make(chan struct{})
	n.e.processors = []*pipelineProcessor{newPipelineProcessor("wait", []pipelineEvent{pipelineCaptured}, false, func(ctx context.Context, _ *processingContext) error {
		close(entered)
		select {
		case <-proceed:
			return nil
		case <-ctx.Done():
			return ctx.Err()
		}
	})}
	start(t, n)
	host := newHost(t, n.e)
	host.must(ObserveBaseline, "baseline")
	h := importText(t, n.e, "old work")
	old := Observation{Epoch: host.epoch, Sequence: 2, Mode: ObserveChange, LeaseID: h.LeaseID}
	done := make(chan error, 1)
	go func() { _, err := n.e.ObserveClipboard(bg, old); done <- err }()
	<-entered
	// This must finish before the processor is released.
	if r, err := n.e.ObserveClipboard(bg, Observation{Epoch: "new-epoch", Sequence: 1, Mode: ObserveBaseline}); err != nil || r.Disposition != DispBaseline {
		t.Fatalf("new baseline blocked or rejected: %v %v", r, err)
	}
	close(proceed)
	if err := <-done; err != nil {
		t.Fatal(err)
	}
	_ = n.e.ReleaseContent(bg, h.LeaseID)
	if r, err := n.e.ObserveClipboard(bg, Observation{Epoch: "new-epoch", Sequence: 2, Mode: ObserveChange}); err != nil || r.Disposition != DispUnchanged {
		t.Fatalf("old result overwrote new baseline: %v %v", r, err)
	}
}

func TestObservationFailureRetryOutlivesSubmissionCache(t *testing.T) {
	n := newContentNode(t, withHost)
	entered := make(chan struct{})
	var calls atomic.Int32
	n.e.processors = []*pipelineProcessor{newPipelineProcessor("cancel", []pipelineEvent{pipelineCaptured}, false, func(ctx context.Context, _ *processingContext) error {
		calls.Add(1)
		close(entered)
		<-ctx.Done()
		return ctx.Err()
	})}
	start(t, n)
	host := newHost(t, n.e)
	host.must(ObserveBaseline, "")
	h := importText(t, n.e, "input")
	o := Observation{Epoch: host.epoch, Sequence: 2, Mode: ObserveChange, LeaseID: h.LeaseID}
	done := make(chan error, 1)
	go func() { _, err := n.e.ObserveClipboard(bg, o); done <- err }()
	<-entered
	// Stop cancellation is cached even after the waiting caller departs.
	if err := n.e.Stop(bg); err != nil {
		t.Fatal(err)
	}
	if err := <-done; code(err) != CodeCancelled {
		t.Fatal(err)
	}
	_ = n.e.ReleaseContent(bg, h.LeaseID)
	n.e.mu.Lock()
	for _, entry := range n.e.submissions {
		entry.finished = time.Now().Add(-2 * n.e.res.SubmissionTTL)
	}
	n.e.pruneSubmissionsLocked(time.Now())
	n.e.mu.Unlock()
	if _, err := n.e.ObserveClipboard(bg, o); code(err) != CodeCancelled {
		t.Fatalf("last observation lost its result: %v", err)
	}
	if calls.Load() != 1 || stored(n.e) != 0 {
		t.Fatal("failed observation repeated or leaked")
	}
}

func TestSubmissionResultSurvivesOperationEviction(t *testing.T) {
	n := newContentNode(t, nil)
	start(t, n)
	l := newLegacy(t, n)
	h := importText(t, n.e, "once")
	req := SubmitRequest{RequestID: "independent", LeaseID: h.LeaseID, Origin: OriginExplicit}
	id, err := n.e.SubmitContent(bg, req)
	if err != nil {
		t.Fatal(err)
	}
	waitSend(t, n.e, id)
	_ = n.e.ReleaseContent(bg, h.LeaseID)
	n.e.mu.Lock()
	n.e.ops.drop(n.e.ops.byID[id]) // simulate operation TTL/capacity eviction
	n.e.mu.Unlock()
	if retry, err := n.e.SubmitContent(bg, req); err != nil || retry != id {
		t.Fatalf("operation eviction lost submission result: %s %v", retry, err)
	}
	if l.count() != 1 {
		t.Fatal("retry dispatched twice")
	}
}

func TestObservationReversionWhileDifferentInputPending(t *testing.T) {
	n := newContentNode(t, withHost)
	entered, proceed := make(chan struct{}), make(chan struct{})
	var calls atomic.Int32
	n.e.processors = []*pipelineProcessor{newPipelineProcessor("wait", []pipelineEvent{pipelineCaptured}, false, func(ctx context.Context, _ *processingContext) error {
		if calls.Add(1) == 1 {
			close(entered)
			select {
			case <-proceed:
			case <-ctx.Done():
				return ctx.Err()
			}
		}
		return nil
	})}
	start(t, n)
	host := newHost(t, n.e)
	host.must(ObserveBaseline, "original")
	h := importText(t, n.e, "different")
	first := Observation{Epoch: host.epoch, Sequence: 2, Mode: ObserveChange, LeaseID: h.LeaseID}
	done := make(chan error, 1)
	go func() { _, err := n.e.ObserveClipboard(bg, first); done <- err }()
	<-entered
	original := importText(t, n.e, "original")
	// Inspect admission separately so the test can release the earlier processor
	// deterministically after the newer sequence has been accepted.
	n.e.obsMu.Lock()
	n.e.mu.Lock()
	record, failure := n.e.admitObservationLocked(Observation{Epoch: host.epoch, Sequence: 3, Mode: ObserveChange, LeaseID: original.LeaseID})
	n.e.mu.Unlock()
	n.e.obsMu.Unlock()
	if failure != nil {
		t.Fatal(failure)
	}
	close(proceed)
	if err := <-done; err != nil {
		t.Fatal(err)
	}
	if r, err := waitObservation(bg, record); err != nil || r.Disposition != DispNoTargets {
		t.Fatalf("reversion suppressed: %v %v", r, err)
	}
	_ = n.e.ReleaseContent(bg, h.LeaseID)
	_ = n.e.ReleaseContent(bg, original.LeaseID)
	host.seq = 3
	if calls.Load() != 2 || host.must(ObserveChange, "original") != DispUnchanged {
		t.Fatal("reversion processing/baseline mismatch")
	}
}

func TestCommitContentSeparatesCommitFromSync(t *testing.T) {
	n := newContentNode(t, nil)
	var calls atomic.Int32
	n.e.processors = []*pipelineProcessor{newPipelineProcessor("count", []pipelineEvent{pipelineCaptured}, false, func(context.Context, *processingContext) error {
		calls.Add(1)
		return nil
	})}
	start(t, n)
	h := importText(t, n.e, "input")
	req := CommitRequest{RequestID: "commit-1", LeaseID: h.LeaseID}
	r, err := n.e.CommitContent(bg, req)
	if err != nil {
		t.Fatalf("a commit without targets must not fail: %v", err)
	}
	if r.Phase != SubmissionCommitted || r.SyncDisposition != SyncNoTargets || r.ItemID == "" || r.OperationID != "" || r.Error != nil {
		t.Fatalf("unexpected result %+v", r)
	}
	// The old facade shares the record and keeps its error contract.
	_ = n.e.ReleaseContent(bg, h.LeaseID)
	if _, err := n.e.SubmitContent(bg, SubmitRequest{RequestID: "commit-1", LeaseID: h.LeaseID, Origin: OriginExplicit}); code(err) != CodeNoTargets {
		t.Fatalf("facade: %v", err)
	}
	got, err := n.e.GetSubmission(bg, "commit-1")
	if err != nil || got != r {
		t.Fatalf("GetSubmission = %+v, %v", got, err)
	}
	if calls.Load() != 1 {
		t.Fatalf("processors ran %d times", calls.Load())
	}
	if _, err := n.e.GetSubmission(bg, "missing"); code(err) != CodeNotFound {
		t.Fatal(err)
	}
	if _, err := n.e.GetSubmission(bg, "obs:x"); code(err) != CodeInvalidArgument {
		t.Fatal(err)
	}
}

func TestCommitContentSyncDisabledAndReservedFields(t *testing.T) {
	n := newContentNode(t, nil)
	start(t, n)
	updateSettings(t, n.e, func(s *Settings) { s.Sync.Enabled = false })
	h := importText(t, n.e, "input")
	r, err := n.e.CommitContent(bg, CommitRequest{RequestID: "commit-off", LeaseID: h.LeaseID})
	if err != nil || r.Phase != SubmissionCommitted || r.SyncDisposition != SyncDisabled {
		t.Fatalf("%+v, %v", r, err)
	}
	if _, err := n.e.SubmitContent(bg, SubmitRequest{RequestID: "commit-off", LeaseID: h.LeaseID, Origin: OriginExplicit}); code(err) != CodeSyncDisabled {
		t.Fatal(err)
	}
	if _, err := n.e.CommitContent(bg, CommitRequest{RequestID: "g", LeaseID: h.LeaseID, WritebackGuard: &WritebackGuard{Epoch: "e"}}); code(err) != CodeUnsupported {
		t.Fatal(err)
	}
	if _, err := n.e.CommitContent(bg, CommitRequest{RequestID: "b", LeaseID: h.LeaseID, BackendIDs: []PluginID{}}); code(err) != CodeNotFound {
		t.Fatal(err)
	}
	if _, err := n.e.CommitContent(bg, CommitRequest{RequestID: "recv:1", LeaseID: h.LeaseID}); code(err) != CodeInvalidArgument {
		t.Fatal(err)
	}
}

package engine

import (
	"context"
	"sync"
	"sync/atomic"
	"testing"
)

// fakeBackend is a contract-test backend. It accepts the configured targets,
// remembers the views it was offered and lets the test drive target progress.
type fakeBackend struct {
	id      PluginID
	pol     BackendSyncPolicy // applied through settings by multiNode
	targets []string
	fail    *Error
	syncEnd bool // report sent from inside Submit, before later backends register

	mu    sync.Mutex
	views []backendView
	host  backendHost
	keys  []targetKey
	calls atomic.Int32
}

func (f *fakeBackend) ID() PluginID        { return f.id }
func (f *fakeBackend) Kind() TransportKind { return TransportPeerSessions }
func (f *fakeBackend) Submit(_ context.Context, v backendView, h backendHost) (bool, *Error) {
	f.calls.Add(1)
	if f.fail != nil {
		return false, f.fail
	}
	if len(f.targets) == 0 {
		return false, nil
	}
	keys := make([]targetKey, len(f.targets))
	for i, t := range f.targets {
		keys[i] = targetKey{f.id, TargetDevice, t}
	}
	if err := h.Register(keys); err != nil {
		return false, err
	}
	f.mu.Lock()
	f.views, f.host, f.keys = append(f.views, v), h, keys
	f.mu.Unlock()
	if f.syncEnd {
		for _, k := range keys {
			h.Report(k, TargetSent, nil)
		}
	}
	return true, nil
}

func (f *fakeBackend) finish(st TargetState) {
	f.mu.Lock()
	h, keys := f.host, f.keys
	f.mu.Unlock()
	for _, k := range keys {
		h.Report(k, st, nil)
	}
}

func (f *fakeBackend) lastView(t *testing.T) backendView {
	t.Helper()
	f.mu.Lock()
	defer f.mu.Unlock()
	if len(f.views) == 0 {
		t.Fatalf("%s was never offered anything", f.id)
	}
	return f.views[len(f.views)-1]
}

var allTypes = DefaultBackendPolicy()

func multiNode(t *testing.T, backs ...*fakeBackend) node {
	n := newContentNode(t, func(o *Options) {
		for _, b := range backs {
			o.extraBackends = append(o.extraBackends, b)
		}
	})
	start(t, n)
	// A backend's policy is Engine's business: apply each fake's wish via settings.
	s := state(t, n.e).Settings
	for _, f := range backs {
		s.Value.BackendPolicies[f.id] = f.pol
	}
	if _, err := n.e.UpdateSettings(bg, UpdateSettingsRequest{ExpectedRevision: s.Revision, Value: s.Value}); err != nil {
		t.Fatal(err)
	}
	return n
}

func textAndFile(t *testing.T, e *Engine) ContentHandle {
	t.Helper()
	h, err := e.ImportContent(bg, ContentInput{
		Representations: []RepresentationInput{rep(MimeText, "words")},
		Files:           []FileInput{fileIn("big.bin", "0123456789ABCDEF")}, // 16 bytes
	})
	if err != nil {
		t.Fatal(err)
	}
	return h
}

func resultOf(r SubmissionResult, id PluginID) BackendSubmissionResult {
	for _, b := range r.BackendResults {
		if b.BackendID == id {
			return b
		}
	}
	return BackendSubmissionResult{}
}

func kinds(v backendView) (reps, files int) {
	for _, p := range v.set.parts {
		if p.desc.Kind == PartFile {
			files++
		} else {
			reps++
		}
	}
	return
}

func TestBackendsGetIndependentViewsAndThresholds(t *testing.T) {
	a := &fakeBackend{id: "test.a", pol: BackendSyncPolicy{Text: true}, targets: []string{"d1"}}
	b := &fakeBackend{id: "test.b", pol: BackendSyncPolicy{Text: true, Images: true, Files: true, AutoFileMaxBytes: 10}, targets: []string{"repo"}}
	n := multiNode(t, a, b)
	var runs atomic.Int32
	n.e.processors = []*pipelineProcessor{newPipelineProcessor("count", []pipelineEvent{pipelineCaptured}, false, func(context.Context, *processingContext) error {
		runs.Add(1)
		return nil
	})}
	h := textAndFile(t, n.e)

	// Manual send crosses the file threshold; each backend sees only what its policy allows.
	r, err := n.e.CommitContent(bg, CommitRequest{RequestID: "man", LeaseID: h.LeaseID})
	if err != nil || r.SyncDisposition != SyncQueued || r.OperationID == "" {
		t.Fatalf("%+v %v", r, err)
	}
	if len(r.BackendResults) != 3 || r.BackendResults[0].BackendID != BackendTailcat || r.BackendResults[0].Disposition != BackendNoTargets {
		t.Fatalf("results = %+v", r.BackendResults)
	}
	if resultOf(r, "test.a").Disposition != BackendQueued || resultOf(r, "test.b").Disposition != BackendQueued {
		t.Fatalf("results = %+v", r.BackendResults)
	}
	if reps, files := kinds(a.lastView(t)); reps != 1 || files != 0 {
		t.Fatalf("a saw %d reps %d files", reps, files)
	}
	if reps, files := kinds(b.lastView(t)); reps != 1 || files != 1 {
		t.Fatalf("b saw %d reps %d files", reps, files)
	}
	if a.lastView(t).id != b.lastView(t).id || a.lastView(t).id == "" {
		t.Fatal("backends must share one transfer identity per commit")
	}
	if runs.Load() != 1 {
		t.Fatalf("processors ran %d times for two backends", runs.Load())
	}
	op, _ := n.e.GetOperation(bg, r.OperationID)
	if len(op.Targets) != 2 || op.Targets[0].BackendID == op.Targets[1].BackendID {
		t.Fatalf("targets = %+v", op.Targets)
	}
	a.finish(TargetSent)
	if op, _ = n.e.GetOperation(bg, r.OperationID); op.Phase == PhaseCompleted {
		t.Fatal("completed while another backend's target was still sending")
	}
	b.finish(TargetFailed)
	op, _ = n.e.GetOperation(bg, r.OperationID)
	if op.Phase != PhaseCompleted || op.Outcome != OutcomePartial {
		t.Fatalf("op = %+v", op)
	}

	// An automatic send is held back from b by its threshold, but not from a.
	h2 := textAndFile(t, n.e)
	res, err := n.e.submit(bg, SubmitRequest{RequestID: "auto", LeaseID: h2.LeaseID, Origin: OriginClipboard})
	if err != nil || res == "" {
		t.Fatal(res, err)
	}
	n.e.mu.Lock()
	entry := n.e.submissions["auto"]
	n.e.mu.Unlock()
	got := entry.result("auto")
	if resultOf(got, "test.b").Disposition != BackendAutoFileTooLarge || resultOf(got, "test.a").Disposition != BackendQueued {
		t.Fatalf("auto results = %+v", got.BackendResults)
	}
	if b.calls.Load() != 1 {
		t.Fatal("a backend skipped by its threshold must not be offered the clip")
	}
}

func TestBackendFailuresAreIsolated(t *testing.T) {
	bad := &fakeBackend{id: "test.bad", pol: allTypes, fail: newErr(CodeUnavailable, "boom")}
	good := &fakeBackend{id: "test.good", pol: allTypes, targets: []string{"x"}}
	n := multiNode(t, bad, good)
	h := importText(t, n.e, "hello")
	r, err := n.e.CommitContent(bg, CommitRequest{RequestID: "iso", LeaseID: h.LeaseID})
	if err != nil || r.Phase != SubmissionCommitted || r.SyncDisposition != SyncQueued {
		t.Fatalf("%+v %v", r, err)
	}
	if b := resultOf(r, "test.bad"); b.Disposition != BackendFailed || b.Error == nil || b.Error.Code != CodeUnavailable {
		t.Fatalf("bad = %+v", b)
	}
	if r.SyncError != nil || r.HistoryID == "" {
		t.Fatalf("one failed backend must not fail the commit: %+v", r)
	}
	op, _ := n.e.GetOperation(bg, r.OperationID)
	if len(op.Targets) != 1 || op.Targets[0].BackendID != "test.good" {
		t.Fatalf("targets = %+v", op.Targets)
	}
}

func TestAggregateWithoutAnyTask(t *testing.T) {
	filesOnly := &fakeBackend{id: "test.files", pol: BackendSyncPolicy{Files: true}, targets: []string{"x"}}
	n := multiNode(t, filesOnly)
	h := importText(t, n.e, "text only")
	r, err := n.e.CommitContent(bg, CommitRequest{RequestID: "pf", LeaseID: h.LeaseID})
	if err != nil || r.OperationID != "" {
		t.Fatalf("%+v %v", r, err)
	}
	// tailcat has no peers (no_targets); the files-only backend filtered the text out.
	if resultOf(r, "test.files").Disposition != BackendPolicyFiltered || resultOf(r, BackendTailcat).Disposition != BackendNoTargets {
		t.Fatalf("%+v", r.BackendResults)
	}
	if r.SyncDisposition != SyncNoTargets {
		t.Fatalf("aggregate = %s, want no_targets (it outranks policy_filtered)", r.SyncDisposition)
	}
	if filesOnly.calls.Load() != 0 {
		t.Fatal("a backend whose policy filtered everything must not be offered the clip")
	}
}

func TestGlobalSwitchStopsEveryBackend(t *testing.T) {
	a := &fakeBackend{id: "test.a", pol: allTypes, targets: []string{"x"}}
	n := multiNode(t, a)
	updateSettings(t, n.e, func(s *Settings) { s.Sync.Enabled = false })
	h := importText(t, n.e, "x")
	r, err := n.e.CommitContent(bg, CommitRequest{RequestID: "off", LeaseID: h.LeaseID})
	if err != nil || r.SyncDisposition != SyncDisabled || r.HistoryID == "" {
		t.Fatalf("%+v %v", r, err)
	}
	for _, b := range r.BackendResults {
		if b.Disposition != BackendDisabled {
			t.Fatalf("%+v", r.BackendResults)
		}
	}
	if a.calls.Load() != 0 {
		t.Fatal("a backend was offered content while sync was off")
	}
}

func TestBackendSelection(t *testing.T) {
	a := &fakeBackend{id: "test.a", pol: allTypes, targets: []string{"x"}}
	b := &fakeBackend{id: "test.b", pol: allTypes, targets: []string{"y"}}
	n := multiNode(t, a, b)
	h := importText(t, n.e, "pick")
	for name, ids := range map[string][]PluginID{"empty": {}, "unknown": {"nope"}, "repeat": {"test.a", "test.a"}} {
		if _, err := n.e.CommitContent(bg, CommitRequest{RequestID: RequestID("sel-" + name), LeaseID: h.LeaseID, BackendIDs: ids}); code(err) != CodeInvalidArgument {
			t.Fatalf("%s: %v", name, err)
		}
	}
	r, err := n.e.CommitContent(bg, CommitRequest{RequestID: "sel", LeaseID: h.LeaseID, BackendIDs: []PluginID{"test.b"}})
	if err != nil || len(r.BackendResults) != 1 || r.BackendResults[0].BackendID != "test.b" {
		t.Fatalf("%+v %v", r, err)
	}
	if a.calls.Load() != 0 || b.calls.Load() != 1 {
		t.Fatalf("calls a=%d b=%d", a.calls.Load(), b.calls.Load())
	}
	// The selection is part of the request's identity.
	if _, err := n.e.CommitContent(bg, CommitRequest{RequestID: "sel", LeaseID: h.LeaseID}); code(err) != CodeConflict {
		t.Fatalf("changed selection under the same RequestID: %v", err)
	}
	if again, err := n.e.CommitContent(bg, CommitRequest{RequestID: "sel", LeaseID: h.LeaseID, BackendIDs: []PluginID{"test.b"}}); err != nil || again.OperationID != r.OperationID {
		t.Fatalf("%+v %v", again, err)
	}
}

func TestOperationWaitsForLateRegistrations(t *testing.T) {
	// a's target is final before b registers: the operation must not complete early.
	a := &fakeBackend{id: "test.a", pol: allTypes, targets: []string{"x"}, syncEnd: true}
	b := &fakeBackend{id: "test.b", pol: allTypes, targets: []string{"y"}}
	n := multiNode(t, a, b)
	h := importText(t, n.e, "late")
	r, err := n.e.CommitContent(bg, CommitRequest{RequestID: "late", LeaseID: h.LeaseID})
	if err != nil {
		t.Fatal(err)
	}
	op, _ := n.e.GetOperation(bg, r.OperationID)
	if op.Phase == PhaseCompleted || len(op.Targets) != 2 {
		t.Fatalf("op = %+v", op)
	}
	b.finish(TargetSent)
	op, _ = n.e.GetOperation(bg, r.OperationID)
	if op.Phase != PhaseCompleted || op.Outcome != OutcomeSuccess {
		t.Fatalf("op = %+v", op)
	}
	// Content is released once the operation completed and the lease is gone.
	_ = n.e.ReleaseContent(bg, h.LeaseID)
	eventually(t, "content freed", func() bool { return stored(n.e) == 0 })
}

func TestBackendPoliciesAreEffectiveValidatedAndHotUpdated(t *testing.T) {
	a := &fakeBackend{id: "test.a", pol: allTypes, targets: []string{"x"}}
	n := multiNode(t, a)
	s := state(t, n.e).Settings
	// The read-back map is complete and is a copy.
	if len(s.Value.BackendPolicies) != 2 || s.Value.BackendPolicies[BackendTailcat] != DefaultBackendPolicy() {
		t.Fatalf("effective policies = %+v", s.Value.BackendPolicies)
	}
	s.Value.BackendPolicies["test.a"] = BackendSyncPolicy{}
	if state(t, n.e).Settings.Value.BackendPolicies["test.a"] != DefaultBackendPolicy() {
		t.Fatal("mutating a returned map changed the engine's settings")
	}
	// Validation: unknown backend, negative or oversized threshold. Nothing changes on error.
	for name, mut := range map[string]func(*Settings){
		"unknown":  func(v *Settings) { v.BackendPolicies["nope"] = DefaultBackendPolicy() },
		"negative": func(v *Settings) { v.BackendPolicies["test.a"] = BackendSyncPolicy{Text: true, AutoFileMaxBytes: -1} },
		"huge": func(v *Settings) {
			v.BackendPolicies["test.a"] = BackendSyncPolicy{Text: true, AutoFileMaxBytes: 3 << 30}
		},
	} {
		cur := state(t, n.e).Settings
		mut(&cur.Value)
		if _, err := n.e.UpdateSettings(bg, UpdateSettingsRequest{ExpectedRevision: cur.Revision, Value: cur.Value}); code(err) != CodeInvalidArgument {
			t.Fatalf("%s: %v", name, err)
		}
	}
	// A missing entry means the default, and the map comes back complete.
	cur := state(t, n.e).Settings
	delete(cur.Value.BackendPolicies, "test.a")
	got, err := n.e.UpdateSettings(bg, UpdateSettingsRequest{ExpectedRevision: cur.Revision, Value: cur.Value})
	if err != nil || got.Revision != cur.Revision || got.Value.BackendPolicies["test.a"] != DefaultBackendPolicy() {
		t.Fatalf("%+v %v", got, err)
	}
	// Hot update: the next commit sees the new policy, without a restart.
	h := textAndFile(t, n.e)
	if _, err := n.e.UpdateSettings(bg, UpdateSettingsRequest{ExpectedRevision: got.Revision, Value: func() Settings {
		v := got.Value
		v.BackendPolicies = map[PluginID]BackendSyncPolicy{"test.a": {Text: true}}
		return v
	}()}); err != nil {
		t.Fatal(err)
	}
	if _, err := n.e.CommitContent(bg, CommitRequest{RequestID: "hot", LeaseID: h.LeaseID}); err != nil {
		t.Fatal(err)
	}
	if reps, files := kinds(a.lastView(t)); reps != 1 || files != 0 {
		t.Fatalf("saw %d reps %d files after the policy change", reps, files)
	}
}

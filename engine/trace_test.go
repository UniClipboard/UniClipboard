package engine

import (
	"context"
	"fmt"
	"strings"
	"sync"
	"sync/atomic"
	"testing"

	"github.com/uniclipboard/unic/internal/plugins/builtintext"
)

type recordedSpan struct {
	TraceID        string
	SpanID         string
	ParentSpanID   string
	RemoteParent   string
	Name           string
	Kind           TraceSpanKind
	Backend        DiagnosticBackend
	ContentKind    DiagnosticContentKind
	Result         DiagnosticResult
	Code           Code
	ActionCategory string
	Ended          bool
}

type fakeTraceCtxKey struct{}

type fakeTraceHandle struct {
	traceID string
	spanID  string
	sampled bool
}

type recordingTracer struct {
	sampled atomic.Bool
	seq     atomic.Uint64
	mu      sync.Mutex
	spans   []*recordedSpan
}

func newRecordingTracer(sampled bool) *recordingTracer {
	tr := &recordingTracer{}
	tr.sampled.Store(sampled)
	return tr
}

func (tr *recordingTracer) StartSpan(ctx context.Context, spec TraceSpanSpec) (context.Context, func(TraceSpanFinish)) {
	if ctx == nil {
		ctx = context.Background()
	}
	id := tr.seq.Add(1)
	spanID := fmt.Sprintf("%016x", id)
	var traceID, parentID string
	sampled := tr.sampled.Load()

	if spec.Name == "unic.sync.receive" && spec.RemoteParent != "" {
		parts := strings.Split(spec.RemoteParent, "-")
		if len(parts) == 4 && len(parts[1]) == 32 && len(parts[2]) == 16 {
			traceID = parts[1]
			parentID = parts[2]
			sampled = parts[3] == "01"
		}
	} else if spec.Name != "unic.submission" {
		if h, ok := ctx.Value(fakeTraceCtxKey{}).(fakeTraceHandle); ok && h.traceID != "" {
			traceID = h.traceID
			parentID = h.spanID
			sampled = h.sampled
		}
	}
	if traceID == "" {
		traceID = fmt.Sprintf("%032x", id)
	}
	rec := &recordedSpan{
		TraceID:      traceID,
		SpanID:       spanID,
		ParentSpanID: parentID,
		RemoteParent: spec.RemoteParent,
		Name:         spec.Name,
		Kind:         spec.Kind,
		Backend:      spec.Backend,
		ContentKind:  spec.ContentKind,
	}
	next := context.WithValue(ctx, fakeTraceCtxKey{}, fakeTraceHandle{traceID: traceID, spanID: spanID, sampled: sampled})
	return next, func(f TraceSpanFinish) {
		if !sampled {
			return
		}
		tr.mu.Lock()
		rec.Result = f.Result
		rec.Code = f.Code
		if f.Backend != "" {
			rec.Backend = f.Backend
		}
		if f.ContentKind != "" {
			rec.ContentKind = f.ContentKind
		}
		rec.ActionCategory = f.ActionCategory
		rec.Ended = true
		tr.spans = append(tr.spans, rec)
		tr.mu.Unlock()
	}
}

func (tr *recordingTracer) InheritTrace(dst, src context.Context) context.Context {
	if dst == nil {
		dst = context.Background()
	}
	if src == nil {
		return dst
	}
	if h, ok := src.Value(fakeTraceCtxKey{}).(fakeTraceHandle); ok {
		return context.WithValue(dst, fakeTraceCtxKey{}, h)
	}
	return dst
}

func (tr *recordingTracer) TraceParent(ctx context.Context) string {
	if ctx == nil {
		return ""
	}
	h, ok := ctx.Value(fakeTraceCtxKey{}).(fakeTraceHandle)
	if !ok || !h.sampled || h.traceID == "" || h.spanID == "" {
		return ""
	}
	return "00-" + h.traceID + "-" + h.spanID + "-01"
}

func (tr *recordingTracer) snapshot() []*recordedSpan {
	tr.mu.Lock()
	defer tr.mu.Unlock()
	out := make([]*recordedSpan, len(tr.spans))
	copy(out, tr.spans)
	return out
}

func (tr *recordingTracer) reset() {
	tr.mu.Lock()
	tr.spans = nil
	tr.mu.Unlock()
}

type panickingTracer struct{}

func (panickingTracer) StartSpan(context.Context, TraceSpanSpec) (context.Context, func(TraceSpanFinish)) {
	panic("boom start")
}
func (panickingTracer) InheritTrace(context.Context, context.Context) context.Context {
	panic("boom inherit")
}
func (panickingTracer) TraceParent(context.Context) string {
	panic("boom traceparent")
}

func TestTracerObserveActionCrossDeviceAndApply(t *testing.T) {
	trA := newRecordingTracer(true)
	trB := newRecordingTracer(true)

	nodeA := newContentNode(t, func(o *Options) {
		o.Tracer = trA
		o.HostCapabilities = HostCapabilities{Observe: AbilityAvailable, Apply: AbilityAvailable, WriteFormats: []string{MimeText}}
		o.Plugins = append(o.Plugins, PluginInstance{Plugin: builtintext.New()})
	})
	nodeB := newContentNode(t, func(o *Options) {
		o.Tracer = trB
		o.HostCapabilities = HostCapabilities{Observe: AbilityAvailable, Apply: AbilityAvailable, WriteFormats: []string{MimeText}}
	})
	start(t, nodeA)
	start(t, nodeB)
	pairTwo(t, nodeA, nodeB)

	// 1. Baseline, unchanged, empty observations produce ZERO spans.
	baseH := importText(t, nodeA.e, "initial")
	if _, err := nodeA.e.ObserveClipboard(bg, Observation{Epoch: "ep1", Sequence: 1, Mode: ObserveBaseline, LeaseID: baseH.LeaseID}); err != nil {
		t.Fatal(err)
	}
	if _, err := nodeA.e.ObserveClipboard(bg, Observation{Epoch: "ep1", Sequence: 2, Mode: ObserveChange, LeaseID: baseH.LeaseID}); err != nil {
		t.Fatal(err)
	}
	if _, err := nodeA.e.ObserveClipboard(bg, Observation{Epoch: "ep1", Sequence: 3, Mode: ObserveChange}); err != nil {
		t.Fatal(err)
	}
	_ = nodeA.e.ReleaseContent(bg, baseH.LeaseID)
	if got := trA.snapshot(); len(got) != 0 {
		t.Fatalf("expected 0 spans for baseline/unchanged/empty, got %d", len(got))
	}

	// 2. Real clipboard change on A -> unic.observe -> unic.commit -> unic.sync.send -> cross-device unic.sync.receive -> unic.apply on B.
	changeH := importText(t, nodeA.e, "hello cross device")
	res, err := nodeA.e.ObserveClipboard(bg, Observation{Epoch: "ep1", Sequence: 4, Mode: ObserveChange, LeaseID: changeH.LeaseID})
	_ = nodeA.e.ReleaseContent(bg, changeH.LeaseID)
	if err != nil || res.Disposition != DispSubmitted {
		t.Fatalf("ObserveClipboard change: res=%+v err=%v", res, err)
	}
	// Duplicate sequence retry must NOT emit another span.
	if _, err := nodeA.e.ObserveClipboard(bg, Observation{Epoch: "ep1", Sequence: 4, Mode: ObserveChange, LeaseID: changeH.LeaseID}); err != nil {
		t.Fatalf("ObserveClipboard duplicate sequence: %v", err)
	}

	// Wait for B to receive the clip.
	rs := waitReceived(t, nodeB.e, 1)
	rec := rs[0]

	// Apply received clip on B.
	tk, err := nodeB.e.BeginApply(bg, ApplyRequest{ReceiptID: rec.ReceiptID})
	if err != nil {
		t.Fatalf("BeginApply on B: %v", err)
	}
	if err := nodeB.e.ReportApplyResult(bg, ApplyResult{Token: tk.Token, Outcome: ApplyOutcomeApplied}); err != nil {
		t.Fatalf("ReportApplyResult on B: %v", err)
	}
	_ = nodeB.e.ReleaseContent(bg, tk.LeaseID)

	// Wait for A's send span to complete.
	var spansA []*recordedSpan
	eventually(t, "sender spans", func() bool {
		spansA = trA.snapshot()
		return len(spansA) >= 3
	})
	var obsSpan, commitSpan, sendSpan *recordedSpan
	for _, s := range spansA {
		switch s.Name {
		case "unic.observe":
			obsSpan = s
		case "unic.commit":
			commitSpan = s
		case "unic.sync.send":
			sendSpan = s
		}
	}
	if obsSpan == nil || commitSpan == nil || sendSpan == nil {
		t.Fatalf("missing sender spans: obs=%v commit=%v send=%v (all=%d)", obsSpan != nil, commitSpan != nil, sendSpan != nil, len(spansA))
	}
	if obsSpan.ParentSpanID != "" || commitSpan.ParentSpanID != obsSpan.SpanID || sendSpan.ParentSpanID != commitSpan.SpanID {
		t.Fatalf("broken sender parent chain: obs=%+v commit=%+v send=%+v", obsSpan, commitSpan, sendSpan)
	}
	if sendSpan.Kind != TraceSpanClient || sendSpan.Backend != "tailcat" || sendSpan.ContentKind != "text" || sendSpan.Result != "succeeded" {
		t.Fatalf("unexpected send span: %+v", sendSpan)
	}

	spansB := trB.snapshot()
	var recvSpan, applySpan *recordedSpan
	for _, s := range spansB {
		switch s.Name {
		case "unic.sync.receive":
			recvSpan = s
		case "unic.apply":
			applySpan = s
		}
	}
	if recvSpan == nil || applySpan == nil {
		t.Fatalf("missing receiver spans: recv=%v apply=%v", recvSpan != nil, applySpan != nil)
	}
	if recvSpan.TraceID != sendSpan.TraceID || recvSpan.ParentSpanID != sendSpan.SpanID || recvSpan.Kind != TraceSpanServer || recvSpan.Backend != "tailcat" {
		t.Fatalf("cross-device propagation mismatch: send=%+v recv=%+v", sendSpan, recvSpan)
	}
	if applySpan.TraceID != recvSpan.TraceID || applySpan.ParentSpanID != recvSpan.SpanID || applySpan.Result != "succeeded" || applySpan.ContentKind != "text" {
		t.Fatalf("receiver apply span mismatch: recv=%+v apply=%+v", recvSpan, applySpan)
	}

	// 3. Action trace + idempotency on A.
	trA.reset()
	trB.reset()
	page, err := nodeA.e.ListHistory(bg, ListHistoryRequest{Limit: 10})
	if err != nil || len(page.Items) == 0 {
		t.Fatalf("ListHistory: %v", err)
	}
	actReq := RunActionRequest{RequestID: "act-trace-1", Key: "builtin.text/upper", HistoryID: page.Items[0].ID}
	actRes, err := nodeA.e.RunAction(bg, actReq)
	if err != nil {
		t.Fatalf("RunAction: %v", err)
	}
	// Idempotent retry must NOT emit another action or commit span.
	if _, err := nodeA.e.RunAction(bg, actReq); err != nil {
		t.Fatalf("RunAction retry: %v", err)
	}
	var actSpan, actCommitSpan *recordedSpan
	for _, s := range trA.snapshot() {
		switch s.Name {
		case "unic.action":
			if actSpan != nil {
				t.Fatal("duplicate unic.action span on idempotent retry")
			}
			actSpan = s
		case "unic.commit":
			if actCommitSpan != nil {
				t.Fatal("duplicate unic.commit span on idempotent retry")
			}
			actCommitSpan = s
		}
	}
	if actSpan == nil || actCommitSpan == nil {
		t.Fatalf("missing action spans: act=%v commit=%v", actSpan != nil, actCommitSpan != nil)
	}
	if actSpan.ActionCategory != "builtin" || actSpan.Result != "succeeded" || actCommitSpan.ParentSpanID != actSpan.SpanID {
		t.Fatalf("unexpected action spans: act=%+v commit=%+v", actSpan, actCommitSpan)
	}
	// Action result also syncs to B; wait for async send/receive before resetting.
	waitReceived(t, nodeB.e, 2)
	eventually(t, "action sync send span", func() bool {
		for _, s := range trA.snapshot() {
			if s.Name == "unic.sync.send" {
				return true
			}
		}
		return false
	})

	// 4. BeginApplyHistory creates an independent unic.apply span and cancellation closes it cleanly.
	trA.reset()
	histTk, err := nodeA.e.BeginApplyHistory(bg, ApplyHistoryRequest{HistoryID: actRes.HistoryID, ExpectedEpoch: "ep1", ExpectedSequence: 4, UserRestore: true})
	if err != nil {
		t.Fatalf("BeginApplyHistory: %v", err)
	}
	// Baseline observation voids the active apply ticket -> must emit cancelled unic.apply span.
	if _, err := nodeA.e.ObserveClipboard(bg, Observation{Epoch: "ep2", Sequence: 1, Mode: ObserveBaseline}); err != nil {
		t.Fatalf("ObserveClipboard new baseline: %v", err)
	}
	_ = nodeA.e.ReleaseContent(bg, histTk.LeaseID)
	spansAfterCancel := trA.snapshot()
	if len(spansAfterCancel) != 1 || spansAfterCancel[0].Name != "unic.apply" || spansAfterCancel[0].Result != "cancelled" || spansAfterCancel[0].Code != CodeCancelled {
		t.Fatalf("expected single cancelled unic.apply span, got %+v", spansAfterCancel)
	}

	// 5. Unsampled sender sends NO traceparent; receiver stays unsampled when remote parent is absent.
	trA.reset()
	trB.reset()
	trA.sampled.Store(false)
	trB.sampled.Store(false)
	uH := importText(t, nodeA.e, "unsampled-content")
	if _, err := nodeA.e.CommitContent(bg, CommitRequest{RequestID: "unsampled-1", LeaseID: uH.LeaseID}); err != nil {
		t.Fatalf("CommitContent unsampled: %v", err)
	}
	_ = nodeA.e.ReleaseContent(bg, uH.LeaseID)
	waitReceived(t, nodeB.e, 3)
	if len(trA.snapshot()) != 0 || len(trB.snapshot()) != 0 {
		t.Fatalf("unsampled sync emitted spans: A=%d B=%d", len(trA.snapshot()), len(trB.snapshot()))
	}
}

func TestTracerPanicIsolation(t *testing.T) {
	n := newContentNode(t, func(o *Options) {
		o.Tracer = panickingTracer{}
	})
	start(t, n)
	h := importText(t, n.e, "still works")
	defer func() { _ = n.e.ReleaseContent(bg, h.LeaseID) }()
	res, err := n.e.CommitContent(bg, CommitRequest{RequestID: "panic-req-1", LeaseID: h.LeaseID, LocalOnly: true})
	if err != nil || res.Phase != SubmissionCommitted {
		t.Fatalf("panicking tracer broke business commit: res=%+v err=%v", res, err)
	}
}

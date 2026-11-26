package mobile

import (
	"bytes"
	"encoding/json"
	"encoding/pem"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"sync"
	"testing"
	"time"

	collecttrace "go.opentelemetry.io/proto/otlp/collector/trace/v1"
	tracepb "go.opentelemetry.io/proto/otlp/trace/v1"
	"google.golang.org/protobuf/encoding/protojson"
	"google.golang.org/protobuf/proto"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/observability"
	"github.com/uniclipboard/unic/internal/plugins/tailcat"
)

func TestMobileTracesOTLP(t *testing.T) {
	if os.Getenv("UNIC_MOBILE_TRACE_CHILD") != "1" {
		args := []string{"test", "-count=1", "-ldflags", "-X github.com/uniclipboard/unic/internal/observability.traceSampleRatio=1", "-run", "^TestMobileTracesOTLP$", "."}
		cmd := exec.Command("go", args...)
		env := append(os.Environ(), "UNIC_MOBILE_TRACE_CHILD=1")
		if runtime.GOOS == "darwin" {
			env = append(env, "TMPDIR=/private/tmp")
		}
		cmd.Env = env
		if out, err := cmd.CombinedOutput(); err != nil {
			t.Fatalf("mobile trace child test failed: %v\n%s", err, out)
		}
		return
	}

	const token = "phc_mobile_trace_test_token"
	var mu sync.Mutex
	var spans []*tracepb.Span
	var receiverErrs []string

	srv := httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		b, err := io.ReadAll(io.LimitReader(r.Body, 2<<20))
		_ = r.Body.Close()
		if err != nil {
			w.WriteHeader(400)
			return
		}
		if r.URL.Path != "/i/v1/traces" {
			// Accept other signals silently so logs/metrics/analytics do not stall.
			w.WriteHeader(200)
			return
		}
		if r.Header.Get("Authorization") != "Bearer "+token {
			mu.Lock()
			receiverErrs = append(receiverErrs, "missing bearer token")
			mu.Unlock()
			w.WriteHeader(401)
			return
		}
		var req collecttrace.ExportTraceServiceRequest
		if err := proto.Unmarshal(b, &req); err != nil {
			mu.Lock()
			receiverErrs = append(receiverErrs, "invalid protobuf")
			mu.Unlock()
			w.WriteHeader(400)
			return
		}
		decoded, _ := protojson.Marshal(&req)
		for _, canary := range []string{"mobile-secret-body", "mobile-share-secret", "mobile-req-canary", token} {
			if bytes.Contains(decoded, []byte(canary)) {
				mu.Lock()
				receiverErrs = append(receiverErrs, "canary leaked: "+canary)
				mu.Unlock()
				w.WriteHeader(400)
				return
			}
		}
		mu.Lock()
		for _, rs := range req.ResourceSpans {
			want := map[string]string{
				"service.name":    "unic-mobile",
				"service.version": observability.ProductVersion,
				"os.type":         runtime.GOOS,
				"host.arch":       runtime.GOARCH,
			}
			if len(rs.Resource.GetAttributes()) != len(want) {
				receiverErrs = append(receiverErrs, "unexpected resource attribute count")
			}
			for _, a := range rs.Resource.GetAttributes() {
				if v, ok := want[a.Key]; !ok || a.Value.GetStringValue() != v {
					receiverErrs = append(receiverErrs, "unexpected resource attr "+a.Key+"="+a.Value.GetStringValue())
				}
			}
			for _, ss := range rs.ScopeSpans {
				if len(ss.Scope.GetAttributes()) > 0 {
					receiverErrs = append(receiverErrs, "unexpected scope attributes")
				}
				for _, sp := range ss.Spans {
					if len(sp.Events) > 0 || len(sp.Links) > 0 || sp.TraceState != "" || sp.Status.GetMessage() != "" {
						receiverErrs = append(receiverErrs, "unexpected span metadata on "+sp.Name)
					}
					for _, a := range sp.Attributes {
						switch a.Key {
						case "result", "code", "backend", "content_kind", "action_category":
						default:
							receiverErrs = append(receiverErrs, "unexpected span attribute "+a.Key)
						}
					}
					spans = append(spans, sp)
				}
			}
		}
		mu.Unlock()
		w.Header().Set("Content-Type", "application/x-protobuf")
		w.WriteHeader(200)
	}))
	defer srv.Close()

	root := t.TempDir()
	certFile := filepath.Join(root, "root.pem")
	if err := os.WriteFile(certFile, pem.EncodeToMemory(&pem.Block{Type: "CERTIFICATE", Bytes: srv.Certificate().Raw}), 0o600); err != nil {
		t.Fatal(err)
	}
	t.Setenv("SSL_CERT_FILE", certFile)

	oldHost, oldToken := observability.PostHogHost, observability.PostHogToken
	observability.PostHogHost = srv.URL
	observability.PostHogToken = token
	t.Cleanup(func() {
		observability.PostHogHost = oldHost
		observability.PostHogToken = oldToken
	})

	caps := &engine.HostCapabilities{
		Observe:      engine.AbilityAvailable,
		Apply:        engine.AbilityAvailable,
		WriteFormats: []string{engine.MimeText},
	}
	cfgA, _ := json.Marshal(map[string]any{
		"data_dir":          filepath.Join(root, "mobile-a"),
		"device_name":       "iPhone Trace A",
		"host_capabilities": caps,
	})
	cfgB, _ := json.Marshal(map[string]any{
		"data_dir":          filepath.Join(root, "mobile-b"),
		"device_name":       "Android Trace B",
		"host_capabilities": caps,
	})

	engA, err := newEngineWithBackend(string(cfgA), tailcat.NewOn(newLoopbackTransport(t)))
	if err != nil {
		t.Fatalf("newEngineWithBackend A: %v", err)
	}
	engB, err := newEngineWithBackend(string(cfgB), tailcat.NewOn(newLoopbackTransport(t)))
	if err != nil {
		_ = engA.Close(nil)
		t.Fatalf("newEngineWithBackend B: %v", err)
	}

	sinkA, sinkB := &testSink{}, &testSink{}
	snapAJSON, err := engA.Subscribe(sinkA)
	if err != nil {
		t.Fatal(err)
	}
	snapBJSON, err := engB.Subscribe(sinkB)
	if err != nil {
		t.Fatal(err)
	}
	var snapA, snapB engine.State
	_ = json.Unmarshal([]byte(snapAJSON), &snapA)
	_ = json.Unmarshal([]byte(snapBJSON), &snapB)

	if err := engA.Start(nil); err != nil {
		t.Fatal(err)
	}
	if err := engB.Start(nil); err != nil {
		t.Fatal(err)
	}

	// Pair A and B.
	tkJSON, err := engA.ExportPairingTicket(nil)
	if err != nil {
		t.Fatal(err)
	}
	var tk struct {
		Ticket string `json:"ticket"`
	}
	_ = json.Unmarshal([]byte(tkJSON), &tk)
	joinReq, _ := json.Marshal(map[string]any{"request_id": "join-mobile-trace", "ticket": tk.Ticket})
	opID, err := engB.JoinByTicket(nil, string(joinReq))
	if err != nil {
		t.Fatal(err)
	}
	eventuallyMobile(t, "mobile join succeeded", func() bool {
		raw, err := engB.GetOperation(opID)
		if err != nil {
			return false
		}
		var op engine.Operation
		_ = json.Unmarshal([]byte(raw), &op)
		return op.Phase == engine.PhaseSucceeded
	})
	approve := func(owner *Engine, peerID engine.DeviceID) {
		var peer engine.Device
		eventuallyMobile(t, "pending peer", func() bool {
			raw, _ := owner.ListDevices()
			var ds []engine.Device
			_ = json.Unmarshal([]byte(raw), &ds)
			for _, d := range ds {
				if d.ID == string(peerID) && len(d.Bindings) > 0 {
					peer = d
					return true
				}
			}
			return false
		})
		bs := make([]map[string]any, len(peer.Bindings))
		for i, b := range peer.Bindings {
			bs[i] = map[string]any{"id": b.ID, "expected_revision": b.Revision}
		}
		req, _ := json.Marshal(map[string]any{
			"id":                   peer.ID,
			"expected_revision":    peer.Revision,
			"expected_fingerprint": peer.Fingerprint,
			"bindings":             bs,
		})
		if _, err := owner.ApproveDevice(string(req)); err != nil {
			t.Fatal(err)
		}
	}
	approve(engA, snapB.Self.ID)
	approve(engB, snapA.Self.ID)
	eventuallyMobile(t, "mobile peers connected", func() bool {
		raw, _ := engA.ListDevices()
		var ds []engine.Device
		_ = json.Unmarshal([]byte(raw), &ds)
		for _, d := range ds {
			if d.ID == string(snapB.Self.ID) {
				for _, b := range d.Bindings {
					if b.Connection == engine.Connected {
						return true
					}
				}
			}
		}
		return false
	})

	// 1. Mobile explicit CommitContent (with idempotent retry) -> cross-device sync -> BeginApply + ReportApplyResult on B.
	hJSON, err := engA.ImportText("mobile-secret-body-1", "", "")
	if err != nil {
		t.Fatal(err)
	}
	var h engine.ContentHandle
	_ = json.Unmarshal([]byte(hJSON), &h)
	commitReq, _ := json.Marshal(map[string]any{"request_id": "mobile-req-canary-1", "lease_id": h.LeaseID})
	subJSON, err := engA.CommitContent(nil, string(commitReq))
	if err != nil {
		t.Fatal(err)
	}
	// Idempotent retry must not create a second submission or commit span.
	if _, err := engA.CommitContent(nil, string(commitReq)); err != nil {
		t.Fatal(err)
	}
	_ = engA.ReleaseContent(string(h.LeaseID))
	var sub engine.SubmissionResult
	_ = json.Unmarshal([]byte(subJSON), &sub)

	var recv engine.ReceivedContent
	eventuallyMobile(t, "B receives mobile commit", func() bool {
		raw, _ := engB.ListReceived()
		var rs []engine.ReceivedContent
		_ = json.Unmarshal([]byte(raw), &rs)
		if len(rs) > 0 {
			recv = rs[0]
			return true
		}
		return false
	})
	applyReq, _ := json.Marshal(map[string]any{"receipt_id": recv.ReceiptID})
	tkRaw, err := engB.BeginApply(string(applyReq))
	if err != nil {
		t.Fatal(err)
	}
	var applyTk engine.ApplyTicket
	_ = json.Unmarshal([]byte(tkRaw), &applyTk)
	repReq, _ := json.Marshal(map[string]any{"token": applyTk.Token, "outcome": engine.ApplyOutcomeApplied})
	if err := engB.ReportApplyResult(string(repReq)); err != nil {
		t.Fatal(err)
	}
	_ = engB.ReleaseContent(string(applyTk.LeaseID))

	// 2. Mobile ObserveClipboard (baseline + real change).
	obsBaseH, _ := engA.ImportText("mobile-base", "", "")
	var ob engine.ContentHandle
	_ = json.Unmarshal([]byte(obsBaseH), &ob)
	if _, err := engA.ObserveClipboard(`{"epoch":"mep1","sequence":"1","mode":"baseline","lease_id":"` + string(ob.LeaseID) + `"}`); err != nil {
		t.Fatal(err)
	}
	_ = engA.ReleaseContent(string(ob.LeaseID))
	obsChH, _ := engA.ImportText("mobile-secret-body-2", "", "")
	var oc engine.ContentHandle
	_ = json.Unmarshal([]byte(obsChH), &oc)
	if _, err := engA.ObserveClipboard(`{"epoch":"mep1","sequence":"2","mode":"change","lease_id":"` + string(oc.LeaseID) + `"}`); err != nil {
		t.Fatal(err)
	}
	_ = engA.ReleaseContent(string(oc.LeaseID))
	eventuallyMobile(t, "B receives observed clipboard", func() bool {
		raw, _ := engB.ListReceived()
		var rs []engine.ReceivedContent
		_ = json.Unmarshal([]byte(raw), &rs)
		return len(rs) >= 2
	})

	// 3. Mobile RunAction (with idempotent retry).
	actReq, _ := json.Marshal(map[string]any{
		"request_id": "mobile-act-1",
		"key":        "builtin.text/upper",
		"history_id": sub.HistoryID,
	})
	if _, err := engA.RunAction(nil, string(actReq)); err != nil {
		t.Fatal(err)
	}
	if _, err := engA.RunAction(nil, string(actReq)); err != nil {
		t.Fatal(err)
	}

	// 4. Mobile CommitSharedItem.
	shareDir := filepath.Join(root, "share-1")
	if err := os.MkdirAll(shareDir, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(shareDir, "entry.json"), []byte(`{"kind":"text","text":"mobile-share-secret"}`), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := engA.CommitSharedItem(shareDir, ""); err != nil {
		t.Fatal(err)
	}

	// Allow async send spans to finish, then Close both engines to flush traces.
	time.Sleep(300 * time.Millisecond)
	if err := engA.Close(nil); err != nil {
		t.Fatal(err)
	}
	if err := engB.Close(nil); err != nil {
		t.Fatal(err)
	}

	mu.Lock()
	defer mu.Unlock()
	if len(receiverErrs) > 0 {
		t.Fatalf("OTLP receiver validation errors: %v", receiverErrs)
	}

	byName := map[string][]*tracepb.Span{}
	for _, sp := range spans {
		byName[sp.Name] = append(byName[sp.Name], sp)
	}
	// Expect:
	// - 2 unic.submission (CommitContent + CommitSharedItem; idempotent retry produced no extra)
	// - 1 unic.observe (baseline produced no span)
	// - 1 unic.action (idempotent retry produced no extra)
	// - at least 2 unic.sync.send and 2 unic.sync.receive linked across mobile devices
	// - 1 unic.apply linked to unic.sync.receive
	if len(byName["unic.submission"]) != 2 {
		t.Fatalf("unic.submission count = %d, want 2", len(byName["unic.submission"]))
	}
	if len(byName["unic.observe"]) != 1 {
		t.Fatalf("unic.observe count = %d, want 1", len(byName["unic.observe"]))
	}
	if len(byName["unic.action"]) != 1 {
		t.Fatalf("unic.action count = %d, want 1", len(byName["unic.action"]))
	}
	if len(byName["unic.apply"]) != 1 {
		t.Fatalf("unic.apply count = %d, want 1", len(byName["unic.apply"]))
	}

	// Verify end-to-end distributed chain: submission -> commit -> sync.send -> sync.receive -> apply.
	apply := byName["unic.apply"][0]
	var matchedRecv, matchedSend, matchedCommit, matchedSub *tracepb.Span
	for _, s := range byName["unic.sync.receive"] {
		if bytes.Equal(s.TraceId, apply.TraceId) && bytes.Equal(apply.ParentSpanId, s.SpanId) {
			matchedRecv = s
		}
	}
	if matchedRecv == nil || matchedRecv.Kind != tracepb.Span_SPAN_KIND_SERVER {
		t.Fatalf("unic.apply not linked to mobile unic.sync.receive")
	}
	for _, s := range byName["unic.sync.send"] {
		if bytes.Equal(s.TraceId, matchedRecv.TraceId) && bytes.Equal(matchedRecv.ParentSpanId, s.SpanId) {
			matchedSend = s
		}
	}
	if matchedSend == nil || matchedSend.Kind != tracepb.Span_SPAN_KIND_CLIENT {
		t.Fatalf("mobile unic.sync.receive not linked to mobile unic.sync.send")
	}
	for _, s := range byName["unic.commit"] {
		if bytes.Equal(s.TraceId, matchedSend.TraceId) && bytes.Equal(matchedSend.ParentSpanId, s.SpanId) {
			matchedCommit = s
		}
	}
	if matchedCommit == nil {
		t.Fatalf("mobile unic.sync.send not linked to unic.commit")
	}
	for _, s := range byName["unic.submission"] {
		if bytes.Equal(s.TraceId, matchedCommit.TraceId) && bytes.Equal(matchedCommit.ParentSpanId, s.SpanId) {
			matchedSub = s
		}
	}
	if matchedSub == nil {
		t.Fatalf("mobile unic.commit not linked to unic.submission")
	}
}

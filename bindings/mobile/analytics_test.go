package mobile

import (
	"bytes"
	"encoding/hex"
	"encoding/json"
	"encoding/pem"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/observability"
	"github.com/uniclipboard/unic/internal/plugins/tailcat"
)

type capturedPostHogEvent struct {
	Event      string         `json:"event"`
	DistinctID string         `json:"distinct_id"`
	Properties map[string]any `json:"properties"`
}

type postHogTestReceiver struct {
	server *httptest.Server
	token  string
	mu     sync.Mutex
	events []capturedPostHogEvent
	errs   []string
	reqs   int
}

func newPostHogTestReceiver(t *testing.T, token string, canaries []string) *postHogTestReceiver {
	t.Helper()
	r := &postHogTestReceiver{token: token}
	allowedProps := map[string]bool{
		"schema_version":          true,
		"product":                 true,
		"product_version":         true,
		"os":                      true,
		"arch":                    true,
		"result":                  true,
		"code":                    true,
		"content_kind":            true,
		"backend":                 true,
		"action_category":         true,
		"source_kind":             true,
		"$process_person_profile": true,
		"$geoip_disable":          true,
	}
	r.server = httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, req *http.Request) {
		body, err := io.ReadAll(io.LimitReader(req.Body, 2<<20))
		_ = req.Body.Close()
		if err != nil {
			w.WriteHeader(http.StatusBadRequest)
			return
		}
		if req.URL.Path != "/batch/" {
			// Accept OTLP logs/metrics/traces silently so they do not stall Close.
			w.WriteHeader(http.StatusOK)
			return
		}
		r.mu.Lock()
		r.reqs++
		for _, c := range canaries {
			if bytes.Contains(body, []byte(c)) {
				r.errs = append(r.errs, "canary leaked in analytics batch: "+c)
			}
		}
		var batch struct {
			APIKey string                 `json:"api_key"`
			Events []capturedPostHogEvent `json:"batch"`
		}
		if err := json.Unmarshal(body, &batch); err != nil {
			r.errs = append(r.errs, "invalid analytics batch json: "+err.Error())
		} else {
			if batch.APIKey != token {
				r.errs = append(r.errs, "unexpected api_key")
			}
			for _, ev := range batch.Events {
				if raw, derr := hex.DecodeString(ev.DistinctID); derr != nil || len(raw) != 16 {
					r.errs = append(r.errs, "invalid distinct_id format: "+ev.DistinctID)
				}
				for k := range ev.Properties {
					if !allowedProps[k] {
						r.errs = append(r.errs, "unexpected analytics property: "+k)
					}
				}
				if ev.Properties["$process_person_profile"] != false {
					r.errs = append(r.errs, "missing $process_person_profile=false on "+ev.Event)
				}
				if ev.Properties["$geoip_disable"] != true {
					r.errs = append(r.errs, "missing $geoip_disable=true on "+ev.Event)
				}
				if ev.Properties["schema_version"] != float64(1) {
					r.errs = append(r.errs, "unexpected schema_version on "+ev.Event)
				}
				if ev.Properties["os"] != runtime.GOOS || ev.Properties["arch"] != runtime.GOARCH {
					r.errs = append(r.errs, "unexpected os/arch on "+ev.Event)
				}
				r.events = append(r.events, ev)
			}
		}
		r.mu.Unlock()
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte(`{"status":1}`))
	}))
	t.Cleanup(r.server.Close)

	certFile := filepath.Join(t.TempDir(), "posthog-root.pem")
	if err := os.WriteFile(certFile, pem.EncodeToMemory(&pem.Block{Type: "CERTIFICATE", Bytes: r.server.Certificate().Raw}), 0o600); err != nil {
		t.Fatal(err)
	}
	t.Setenv("SSL_CERT_FILE", certFile)

	oldHost, oldToken := observability.PostHogHost, observability.PostHogToken
	observability.PostHogHost = r.server.URL
	observability.PostHogToken = token
	t.Cleanup(func() {
		observability.PostHogHost = oldHost
		observability.PostHogToken = oldToken
	})
	return r
}

func (r *postHogTestReceiver) snapshot() ([]capturedPostHogEvent, []string, int) {
	r.mu.Lock()
	defer r.mu.Unlock()
	evs := append([]capturedPostHogEvent(nil), r.events...)
	errs := append([]string(nil), r.errs...)
	return evs, errs, r.reqs
}

func TestMobileProductEventsPostHog(t *testing.T) {
	const token = "phc_mobile_analytics_e2e_token"
	canaries := []string{"mobile-analytics-secret-1", "mobile-analytics-secret-2", "mobile-analytics-req-canary", "iPhone Analytics A", "Android Analytics B"}
	recv := newPostHogTestReceiver(t, token, canaries)

	root := t.TempDir()
	dirA := filepath.Join(root, "mobile-a")
	dirB := filepath.Join(root, "mobile-b")
	caps := &engine.HostCapabilities{
		Observe:      engine.AbilityAvailable,
		Apply:        engine.AbilityAvailable,
		WriteFormats: []string{engine.MimeText},
	}
	cfgA, _ := json.Marshal(map[string]any{
		"data_dir":          dirA,
		"device_name":       "iPhone Analytics A",
		"host_capabilities": caps,
	})
	cfgB, _ := json.Marshal(map[string]any{
		"data_dir":          dirB,
		"device_name":       "Android Analytics B",
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

	// Verify telemetry reports analytics active after Start.
	stRaw, err := engA.GetTelemetryState()
	if err != nil {
		t.Fatal(err)
	}
	var tel observability.TelemetryState
	if err := json.Unmarshal([]byte(stRaw), &tel); err != nil {
		t.Fatal(err)
	}
	if tel.Analytics.State != "active" || !tel.Credential {
		t.Fatalf("unexpected analytics telemetry state after Start: %+v", tel)
	}

	// A second Start on the same Engine instance fails and must not emit a duplicate app_started.
	if err := engA.Start(nil); err == nil {
		t.Fatal("expected duplicate Start on running Engine to fail")
	}

	// 1. Pair A and B -> pairing_completed on both sides.
	tkJSON, err := engA.ExportPairingTicket(nil)
	if err != nil {
		t.Fatal(err)
	}
	var tk struct {
		Ticket string `json:"ticket"`
	}
	_ = json.Unmarshal([]byte(tkJSON), &tk)
	joinReq, _ := json.Marshal(map[string]any{"request_id": "join-mobile-analytics", "ticket": tk.Ticket})
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

	// 2. Two explicit sends from A -> manual_send_completed (x2) and remote_content_applied (coalesced to 1/day) on B.
	var firstSub engine.SubmissionResult
	for idx, body := range []string{"mobile-analytics-secret-1", "mobile-analytics-secret-2"} {
		hJSON, err := engA.ImportText(body, "", "")
		if err != nil {
			t.Fatal(err)
		}
		var h engine.ContentHandle
		_ = json.Unmarshal([]byte(hJSON), &h)
		commitReq, _ := json.Marshal(map[string]any{
			"request_id": "mobile-analytics-req-canary-" + string(rune('1'+idx)),
			"lease_id":   h.LeaseID,
		})
		subJSON, err := engA.CommitContent(nil, string(commitReq))
		if err != nil {
			t.Fatal(err)
		}
		_ = engA.ReleaseContent(string(h.LeaseID))
		var sub engine.SubmissionResult
		_ = json.Unmarshal([]byte(subJSON), &sub)
		if idx == 0 {
			firstSub = sub
		}
		eventuallyMobile(t, "send op completed", func() bool {
			raw, err := engA.GetOperation(string(sub.OperationID))
			if err != nil {
				return false
			}
			var op engine.Operation
			_ = json.Unmarshal([]byte(raw), &op)
			return op.Phase == engine.PhaseCompleted
		})
	}

	var received []engine.ReceivedContent
	eventuallyMobile(t, "B receives both clips", func() bool {
		raw, _ := engB.ListReceived()
		var rs []engine.ReceivedContent
		_ = json.Unmarshal([]byte(raw), &rs)
		if len(rs) >= 2 {
			received = rs
			return true
		}
		return false
	})
	for _, rc := range received {
		applyReq, _ := json.Marshal(map[string]any{"receipt_id": rc.ReceiptID})
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
	}

	// 3. User-restored history apply on B -> history_applied (source_kind=remote).
	histRaw, err := engB.ListHistory(`{"limit":10}`)
	if err != nil {
		t.Fatal(err)
	}
	var histPage engine.HistoryPage
	_ = json.Unmarshal([]byte(histRaw), &histPage)
	if len(histPage.Items) == 0 {
		t.Fatal("expected remote history item on B")
	}
	htkRaw, err := engB.BeginApplyHistory(string(histPage.Items[0].ID), "", "", true)
	if err != nil {
		t.Fatal(err)
	}
	var htk engine.ApplyTicket
	_ = json.Unmarshal([]byte(htkRaw), &htk)
	repHist, _ := json.Marshal(map[string]any{"token": htk.Token, "outcome": engine.ApplyOutcomeApplied})
	if err := engB.ReportApplyResult(string(repHist)); err != nil {
		t.Fatal(err)
	}
	_ = engB.ReleaseContent(string(htk.LeaseID))

	// 4. Built-in Action on A -> action_completed (action_category=builtin).
	actReq, _ := json.Marshal(map[string]any{
		"request_id": "mobile-analytics-act-1",
		"key":        "builtin.text/upper",
		"history_id": firstSub.HistoryID,
	})
	if _, err := engA.RunAction(nil, string(actReq)); err != nil {
		t.Fatal(err)
	}

	// Give the daily worker a brief moment to persist the daily markers before Close flushes.
	eventuallyMobile(t, "daily markers persisted on A and B", func() bool {
		_, errBRemote := os.Stat(filepath.Join(dirB, "analytics-remote-day"))
		_, errBActive := os.Stat(filepath.Join(dirB, "analytics-active-day"))
		_, errAActive := os.Stat(filepath.Join(dirA, "analytics-active-day"))
		return errBRemote == nil && errBActive == nil && errAActive == nil
	})

	if err := engA.Close(nil); err != nil {
		t.Fatal(err)
	}
	if err := engB.Close(nil); err != nil {
		t.Fatal(err)
	}

	idA, err := os.ReadFile(filepath.Join(dirA, "analytics-id"))
	if err != nil {
		t.Fatalf("read analytics-id A: %v", err)
	}
	idB, err := os.ReadFile(filepath.Join(dirB, "analytics-id"))
	if err != nil {
		t.Fatalf("read analytics-id B: %v", err)
	}
	if string(idA) == string(idB) {
		t.Fatal("distinct mobile profiles reused the same analytics-id")
	}

	events, errs, _ := recv.snapshot()
	if len(errs) > 0 {
		t.Fatalf("PostHog receiver validation errors: %v", errs)
	}

	counts := map[string]int{}
	for _, ev := range events {
		if ev.Properties["product"] != "mobile" {
			t.Fatalf("event %q had product=%v, want mobile", ev.Event, ev.Properties["product"])
		}
		if ev.DistinctID != string(idA) && ev.DistinctID != string(idB) {
			t.Fatalf("event %q had unknown distinct_id %q", ev.Event, ev.DistinctID)
		}
		counts[ev.Event]++
		switch ev.Event {
		case "history_applied":
			if ev.Properties["source_kind"] != "remote" {
				t.Fatalf("history_applied source_kind=%v, want remote", ev.Properties["source_kind"])
			}
		case "action_completed":
			if ev.Properties["action_category"] != "builtin" || ev.Properties["result"] != "succeeded" {
				t.Fatalf("unexpected action_completed properties: %+v", ev.Properties)
			}
		case "manual_send_completed":
			if ev.Properties["result"] != "succeeded" || ev.Properties["content_kind"] != "text" {
				t.Fatalf("unexpected manual_send_completed properties: %+v", ev.Properties)
			}
		case "remote_content_applied":
			if ev.DistinctID != string(idB) || ev.Properties["result"] != "succeeded" {
				t.Fatalf("unexpected remote_content_applied event: %+v", ev)
			}
		}
	}

	if counts["app_started"] != 2 {
		t.Fatalf("app_started count = %d, want 2 (events=%+v)", counts["app_started"], events)
	}
	if counts["pairing_completed"] != 2 {
		t.Fatalf("pairing_completed count = %d, want 2", counts["pairing_completed"])
	}
	if counts["manual_send_completed"] != 2 {
		t.Fatalf("manual_send_completed count = %d, want 2", counts["manual_send_completed"])
	}
	if counts["remote_content_applied"] != 1 {
		t.Fatalf("remote_content_applied count = %d, want 1 (daily coalesced)", counts["remote_content_applied"])
	}
	if counts["history_applied"] != 1 {
		t.Fatalf("history_applied count = %d, want 1", counts["history_applied"])
	}
	if counts["action_completed"] != 1 {
		t.Fatalf("action_completed count = %d, want 1", counts["action_completed"])
	}
	if counts["daily_active"] != 2 {
		t.Fatalf("daily_active count = %d, want 2 (1 per active profile)", counts["daily_active"])
	}
}

func TestMobileProductEventsLifecycleAndExtensionMode(t *testing.T) {
	const token = "phc_mobile_lifecycle_token"
	recv := newPostHogTestReceiver(t, token, []string{"lifecycle-secret-text"})

	root := t.TempDir()
	profile := filepath.Join(root, "profile")
	if err := os.MkdirAll(profile, 0o700); err != nil {
		t.Fatal(err)
	}

	// 1. Pre-start false: clears stale analytics-id + daily marker and sends zero analytics requests.
	if err := os.WriteFile(filepath.Join(profile, "mobile-telemetry.json"), []byte("false\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(profile, "analytics-id"), []byte(strings.Repeat("a", 32)), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(profile, "analytics-remote-day"), []byte(`{"identity":"`+strings.Repeat("a", 32)+`","day":"2026-01-01"}`), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(profile, "analytics-active-day"), []byte(`{"identity":"`+strings.Repeat("a", 32)+`","day":"2026-01-01"}`), 0o600); err != nil {
		t.Fatal(err)
	}

	caps := &engine.HostCapabilities{
		Observe:      engine.AbilityAvailable,
		Apply:        engine.AbilityAvailable,
		WriteFormats: []string{engine.MimeText},
	}
	mainCfg, _ := json.Marshal(map[string]any{"data_dir": profile, "device_name": "iPhone Lifecycle", "host_capabilities": caps})
	eng, err := newEngineWithBackend(string(mainCfg), tailcat.NewOn(newLoopbackTransport(t)))
	if err != nil {
		t.Fatal(err)
	}
	if err := eng.Start(nil); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(filepath.Join(profile, "analytics-id")); !os.IsNotExist(err) {
		t.Fatalf("stale analytics-id was not cleared on pre-start false: %v", err)
	}
	if _, err := os.Stat(filepath.Join(profile, "analytics-remote-day")); !os.IsNotExist(err) {
		t.Fatalf("stale analytics-remote-day was not cleared on pre-start false: %v", err)
	}
	if _, err := os.Stat(filepath.Join(profile, "analytics-active-day")); !os.IsNotExist(err) {
		t.Fatalf("stale analytics-active-day was not cleared on pre-start false: %v", err)
	}
	stRaw, err := eng.GetTelemetryState()
	if err != nil {
		t.Fatal(err)
	}
	var tel observability.TelemetryState
	_ = json.Unmarshal([]byte(stRaw), &tel)
	if tel.Analytics.State != "disabled" {
		t.Fatalf("analytics.state=%q, want disabled", tel.Analytics.State)
	}
	if err := eng.Close(nil); err != nil {
		t.Fatal(err)
	}
	if evs, _, reqs := recv.snapshot(); len(evs) != 0 || reqs != 0 {
		t.Fatalf("expected zero analytics requests when disabled, got reqs=%d events=%+v", reqs, evs)
	}

	// 2. Re-enable and start main app -> creates analytics-id and emits app_started (product=mobile).
	//    Idle startup + baseline ObserveClipboard must NOT emit daily_active.
	if err := os.WriteFile(filepath.Join(profile, "mobile-telemetry.json"), []byte("true\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	eng, err = newEngineWithBackend(string(mainCfg), tailcat.NewOn(newLoopbackTransport(t)))
	if err != nil {
		t.Fatal(err)
	}
	if err := eng.Start(nil); err != nil {
		t.Fatal(err)
	}
	baseHJSON, err := eng.ImportText("lifecycle-secret-text", "", "")
	if err != nil {
		t.Fatal(err)
	}
	var baseH engine.ContentHandle
	_ = json.Unmarshal([]byte(baseHJSON), &baseH)
	if _, err := eng.ObserveClipboard(`{"epoch":"life-ep","sequence":"1","mode":"baseline","lease_id":"` + string(baseH.LeaseID) + `"}`); err != nil {
		t.Fatal(err)
	}
	_ = eng.ReleaseContent(string(baseH.LeaseID))
	firstIDBytes, err := os.ReadFile(filepath.Join(profile, "analytics-id"))
	if err != nil {
		t.Fatalf("expected analytics-id after enabled start: %v", err)
	}
	firstID := string(firstIDBytes)
	if err := eng.Close(nil); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(filepath.Join(profile, "analytics-active-day")); !os.IsNotExist(err) {
		t.Fatalf("idle startup and baseline observation must not create analytics-active-day: %v", err)
	}

	// 3. Extension mode sharing the same profile reuses analytics-id, emits NO app_started,
	//    and a real ObserveClipboard change + manual CommitContent coalesce into 1 daily_active.
	extCfg, _ := json.Marshal(map[string]any{
		"data_dir":          profile,
		"device_name":       "iPhone Lifecycle",
		"extension_mode":    true,
		"host_capabilities": caps,
	})
	ext, err := newEngineWithBackend(string(extCfg), tailcat.NewOn(newLoopbackTransport(t)))
	if err != nil {
		t.Fatal(err)
	}
	if err := ext.Start(nil); err != nil {
		t.Fatal(err)
	}
	extBaseHJSON, err := ext.ImportText("lifecycle-base", "", "")
	if err != nil {
		t.Fatal(err)
	}
	var extBaseH engine.ContentHandle
	_ = json.Unmarshal([]byte(extBaseHJSON), &extBaseH)
	if _, err := ext.ObserveClipboard(`{"epoch":"ext-ep","sequence":"1","mode":"baseline","lease_id":"` + string(extBaseH.LeaseID) + `"}`); err != nil {
		t.Fatal(err)
	}
	_ = ext.ReleaseContent(string(extBaseH.LeaseID))
	obsHJSON, err := ext.ImportText("lifecycle-secret-text", "", "")
	if err != nil {
		t.Fatal(err)
	}
	var obsH engine.ContentHandle
	_ = json.Unmarshal([]byte(obsHJSON), &obsH)
	if _, err := ext.ObserveClipboard(`{"epoch":"ext-ep","sequence":"2","mode":"change","lease_id":"` + string(obsH.LeaseID) + `"}`); err != nil {
		t.Fatal(err)
	}
	_ = ext.ReleaseContent(string(obsH.LeaseID))
	hJSON, err := ext.ImportText("lifecycle-secret-text", "", "")
	if err != nil {
		t.Fatal(err)
	}
	var h engine.ContentHandle
	_ = json.Unmarshal([]byte(hJSON), &h)
	commitReq, _ := json.Marshal(map[string]any{"request_id": "ext-commit-1", "lease_id": h.LeaseID, "local_only": true})
	if _, err := ext.CommitContent(nil, string(commitReq)); err != nil {
		t.Fatal(err)
	}
	_ = ext.ReleaseContent(string(h.LeaseID))
	eventuallyMobile(t, "analytics-active-day persisted in extension_mode", func() bool {
		_, err := os.Stat(filepath.Join(profile, "analytics-active-day"))
		return err == nil
	})
	if err := ext.Close(nil); err != nil {
		t.Fatal(err)
	}

	// 4. Disable at runtime removes analytics-id + analytics-active-day; re-enable rotates to a
	//    brand new analytics-id, preserves product="mobile", and emits daily_active under the new ID.
	eng, err = newEngineWithBackend(string(mainCfg), tailcat.NewOn(newLoopbackTransport(t)))
	if err != nil {
		t.Fatal(err)
	}
	if err := eng.Start(nil); err != nil {
		t.Fatal(err)
	}
	if _, err := eng.SetTelemetryEnabled(false); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(filepath.Join(profile, "analytics-id")); !os.IsNotExist(err) {
		t.Fatalf("analytics-id remained after SetTelemetryEnabled(false): %v", err)
	}
	if _, err := os.Stat(filepath.Join(profile, "analytics-active-day")); !os.IsNotExist(err) {
		t.Fatalf("analytics-active-day remained after SetTelemetryEnabled(false): %v", err)
	}
	if _, err := eng.SetTelemetryEnabled(true); err != nil {
		t.Fatal(err)
	}
	rotatedIDBytes, err := os.ReadFile(filepath.Join(profile, "analytics-id"))
	if err != nil {
		t.Fatalf("expected rotated analytics-id after re-enable: %v", err)
	}
	rotatedID := string(rotatedIDBytes)
	if rotatedID == firstID {
		t.Fatalf("analytics-id did not rotate after disable/re-enable: %s", rotatedID)
	}
	hJSON2, err := eng.ImportText("lifecycle-secret-text", "", "")
	if err != nil {
		t.Fatal(err)
	}
	var h2 engine.ContentHandle
	_ = json.Unmarshal([]byte(hJSON2), &h2)
	commitReq2, _ := json.Marshal(map[string]any{"request_id": "rotated-commit-1", "lease_id": h2.LeaseID, "local_only": true})
	if _, err := eng.CommitContent(nil, string(commitReq2)); err != nil {
		t.Fatal(err)
	}
	_ = eng.ReleaseContent(string(h2.LeaseID))
	eventuallyMobile(t, "analytics-active-day persisted for rotated ID", func() bool {
		_, err := os.Stat(filepath.Join(profile, "analytics-active-day"))
		return err == nil
	})
	if err := eng.Close(nil); err != nil {
		t.Fatal(err)
	}

	evs, errs, _ := recv.snapshot()
	if len(errs) > 0 {
		t.Fatalf("receiver errors: %v", errs)
	}
	appStartedFirst := 0
	manualFirst := 0
	manualRotated := 0
	dailyFirst := 0
	dailyRotated := 0
	for _, ev := range evs {
		if ev.Properties["product"] != "mobile" {
			t.Fatalf("unexpected product=%v on %+v", ev.Properties["product"], ev)
		}
		if ev.Event == "app_started" && ev.DistinctID == firstID {
			appStartedFirst++
		}
		if ev.Event == "manual_send_completed" && ev.DistinctID == firstID {
			manualFirst++
		}
		if ev.Event == "manual_send_completed" && ev.DistinctID == rotatedID {
			manualRotated++
		}
		if ev.Event == "daily_active" && ev.DistinctID == firstID {
			dailyFirst++
		}
		if ev.Event == "daily_active" && ev.DistinctID == rotatedID {
			dailyRotated++
		}
	}
	// Main app flushed 1 app_started with firstID in step 2; extension_mode (step 3)
	// must NOT emit app_started, and immediate disable in step 4 discards its queued app_started.
	if appStartedFirst != 1 {
		t.Fatalf("app_started count for firstID = %d, want 1 (events=%+v)", appStartedFirst, evs)
	}
	if manualFirst != 1 {
		t.Fatalf("extension_mode manual_send_completed count for firstID = %d, want 1", manualFirst)
	}
	if manualRotated != 1 {
		t.Fatalf("re-enabled manual_send_completed count for rotatedID = %d, want 1", manualRotated)
	}
	if dailyFirst != 1 {
		t.Fatalf("daily_active count for firstID = %d, want 1", dailyFirst)
	}
	if dailyRotated != 1 {
		t.Fatalf("daily_active count for rotatedID = %d, want 1", dailyRotated)
	}
}

func TestMobileProductEventsUnconfiguredDegradesSafely(t *testing.T) {
	oldHost, oldToken := observability.PostHogHost, observability.PostHogToken
	observability.PostHogHost = ""
	observability.PostHogToken = ""
	t.Cleanup(func() {
		observability.PostHogHost = oldHost
		observability.PostHogToken = oldToken
	})

	profile := filepath.Join(t.TempDir(), "unconfigured")
	cfg, _ := json.Marshal(map[string]any{"data_dir": profile, "device_name": "Dev Phone"})
	eng, err := newEngineWithBackend(string(cfg), tailcat.NewOn(newLoopbackTransport(t)))
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = eng.Close(nil) }()
	if err := eng.Start(nil); err != nil {
		t.Fatal(err)
	}

	stRaw, err := eng.GetTelemetryState()
	if err != nil {
		t.Fatal(err)
	}
	var tel observability.TelemetryState
	if err := json.Unmarshal([]byte(stRaw), &tel); err != nil {
		t.Fatal(err)
	}
	if tel.Analytics.State != "degraded" || tel.Analytics.Error == nil || *tel.Analytics.Error != "not_configured" {
		t.Fatalf("expected analytics degraded/not_configured, got %+v", tel.Analytics)
	}
	if _, err := os.Stat(filepath.Join(profile, "analytics-id")); !os.IsNotExist(err) {
		t.Fatalf("analytics-id must not be created when credentials are absent: %v", err)
	}
	time.Sleep(10 * time.Millisecond)
}

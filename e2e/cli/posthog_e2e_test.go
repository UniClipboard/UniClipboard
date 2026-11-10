package e2e

import (
	"encoding/json"
	"fmt"
	"math"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"

	collectmetrics "go.opentelemetry.io/proto/otlp/collector/metrics/v1"
	metricspb "go.opentelemetry.io/proto/otlp/metrics/v1"
	"google.golang.org/protobuf/proto"
)

// These process-level scenarios precede the PostHog runtime implementation.
// The failure inventory is docs/exec/0019-posthog-integration.md section 3.
// No real Cloud host, project, clipboard or GUI is used.
func TestPostHog(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	h := newObservabilityRun(t)
	h.posthog = true
	h.artifactName = "posthog"
	cfg := fmt.Sprintf("[telemetry]\nendpoint=%q\n", h.server.URL+"/i")
	h.run(t, "default-on", func(t *testing.T) {
		h.startConfig(t, cfg)
		h.cli(t, "copy", "posthog-private-clipboard-canary")
		h.eventually(t, func() bool { return h.hasPath("/i/v1/metrics") && len(h.analytics()) > 0 })
		h.cli(t, "logs", "level", "debug", "--target", "remote", "--for", "15m")
		h.request(t, "POST", "/test/logs", "")
		h.eventually(t, func() bool { return h.hasPath("/i/v1/logs") })
		h.contains(t, "/v1/diagnostics/telemetry", `"credential_configured":true`)
		h.contains(t, "/v1/diagnostics/telemetry", `"analytics":{"requested_enabled":true`)
		service := 0
		for _, ev := range h.analytics() {
			if ev["event"] == "app_started" && ev["properties"].(map[string]any)["product"] == "cli" {
				service++
			}
		}
		if service != 1 {
			t.Fatalf("service startup count=%d", service)
		}
		h.assertPostHogPrivacy(t)
	})
	var oldID string
	h.run(t, "disable-barrier-and-identity", func(t *testing.T) {
		oldID = h.analytics()[0]["distinct_id"].(string)
		h.cli(t, "config", "set", "telemetry.enabled", "false")
		h.contains(t, "/v1/diagnostics/telemetry", `"state":"disabled"`)
		h.requireNoRequests(t, 700*time.Millisecond)
		if _, e := os.Stat(filepath.Join(h.home, "analytics-id")); !os.IsNotExist(e) {
			t.Fatalf("identity retained on disable: %v", e)
		}
		h.cli(t, "config", "set", "telemetry.enabled", "true")
		// A real user action produces the new generation's event; startup is not replayed.
		before := len(h.analytics())
		h.cli(t, "history", "action", "@1", "builtin.text/upper", "--request-id", "posthog-action")
		h.eventually(t, func() bool { return len(h.analytics()) > before })
		evs := h.analytics()
		if evs[len(evs)-1]["distinct_id"] == oldID {
			t.Fatal("analytics identity did not rotate")
		}
	})
	h.run(t, "explicit-false-before-restart", func(t *testing.T) {
		h.cli(t, "config", "set", "telemetry.enabled", "false")
		h.stop(t)
		h.startConfig(t, fmt.Sprintf("[telemetry]\nenabled=false\nendpoint=%q\n", h.server.URL+"/i"))
		h.requireNoRequests(t, 700*time.Millisecond)
		h.cli(t, "config", "set", "telemetry.analytics.enabled", "false")
		h.cli(t, "config", "set", "telemetry.enabled", "true")
		h.contains(t, "/v1/diagnostics/telemetry", `"analytics":{"requested_enabled":false`)
	})
	h.run(t, "multi-process-delta", func(t *testing.T) {
		b := newObservabilityRun(t)
		b.posthog = true
		b.artifactName = "posthog-second"
		b.startConfig(t, fmt.Sprintf("[telemetry]\nendpoint=%q\n[telemetry.analytics]\nenabled=false\n", b.server.URL+"/i"))
		h.cli(t, "config", "set", "sync.enabled", "false")
		b.cli(t, "config", "set", "sync.enabled", "false")
		mark := time.Now()
		for i := 0; i < 3; i++ {
			h.cli(t, "copy", fmt.Sprintf("delta-A-%d", i))
		}
		for i := 0; i < 7; i++ {
			b.cli(t, "copy", fmt.Sprintf("delta-B-%d", i))
		}
		h.eventually(t, func() bool { return h.deltaCount(t, mark)+b.deltaCount(t, mark) == 10 })
		h.request(t, "POST", "/test/metric-samples", "")
		b.request(t, "POST", "/test/metric-samples", "")
		h.eventually(t, func() bool { return h.histogramCount(t)+b.histogramCount(t) == 6 })
		time.Sleep(1100 * time.Millisecond)
		if n := h.deltaCount(t, mark) + b.deltaCount(t, mark); n != 10 {
			t.Fatalf("idle collection duplicated operations: %d", n)
		}
	})
	h.run(t, "actions-history-and-idempotency", func(t *testing.T) {
		h.cli(t, "config", "set", "telemetry.analytics.enabled", "true")
		before := len(h.analytics())
		h.cli(t, "history", "action", "@1", "builtin.text/trim", "--request-id", "posthog-once")
		h.cli(t, "history", "action", "@2", "builtin.text/trim", "--request-id", "posthog-once")
		h.cli(t, "history", "use", "@1", "--request-id", "posthog-restore-once")
		h.cli(t, "history", "use", "@1", "--request-id", "posthog-restore-once")
		h.eventually(t, func() bool { return len(h.analytics()) >= before+2 })
		time.Sleep(250 * time.Millisecond)
		actions, restores := 0, 0
		for _, ev := range h.analytics()[before:] {
			if ev["event"] == "action_completed" {
				actions++
			}
			if ev["event"] == "history_applied" {
				restores++
			}
		}
		if actions != 1 || restores != 1 {
			t.Fatalf("retry duplicated events: actions=%d restores=%d", actions, restores)
		}
	})
	h.run(t, "fault-and-disable-inflight", func(t *testing.T) {
		h.mu.Lock()
		h.status = 429
		h.mu.Unlock()
		h.cli(t, "history", "action", "@1", "builtin.text/plain", "--request-id", "posthog-rate-limit")
		h.eventually(t, func() bool {
			return strings.Contains(string(h.get(t, "/v1/diagnostics/telemetry")), `"last_error_code":"rate_limited"`)
		})
		h.mu.Lock()
		h.status = 200
		h.responseDelay = 10 * time.Second
		h.mu.Unlock()
		h.cli(t, "history", "action", "@1", "builtin.text/plain", "--request-id", "posthog-stall")
		time.Sleep(150 * time.Millisecond)
		started := time.Now()
		h.cli(t, "config", "set", "telemetry.enabled", "false")
		if time.Since(started) > 2*time.Second {
			t.Fatal("disable waited for receiver")
		}
		h.requireNoRequests(t, 400*time.Millisecond)
		h.mu.Lock()
		h.responseDelay = 0
		h.mu.Unlock()
	})
	h.run(t, "missing-deployment-and-identity-symlink", func(t *testing.T) {
		h.stop(t)
		h.startConfig(t, "")
		h.contains(t, "/v1/diagnostics/telemetry", `"state":"degraded"`)
		h.requireNoRequests(t, 200*time.Millisecond)
		h.stop(t)
		target := filepath.Join(h.dir, "private-canary")
		if err := os.WriteFile(target, []byte("identity-private-canary"), 0600); err != nil {
			t.Fatal(err)
		}
		if err := os.Symlink(target, filepath.Join(h.home, "analytics-id")); err != nil {
			t.Fatal(err)
		}
		h.startConfig(t, cfg)
		h.contains(t, "/v1/diagnostics/telemetry", `"last_error_code":"identity_unavailable"`)
		if b, err := os.ReadFile(target); err != nil || string(b) != "identity-private-canary" {
			t.Fatal("identity link altered another file")
		}
	})

	h.run(t, "final-privacy-and-artifact", func(t *testing.T) { h.assertPostHogPrivacy(t) })
}
func (h *observabilityRun) analytics() []map[string]any {
	h.mu.Lock()
	defer h.mu.Unlock()
	var events []map[string]any
	for _, r := range h.requests {
		if r.Path == "/batch/" {
			var b struct {
				Batch []map[string]any `json:"batch"`
			}
			if json.Unmarshal(r.Body, &b) == nil {
				events = append(events, b.Batch...)
			}
		}
	}
	return events
}
func (h *observabilityRun) assertPostHogPrivacy(t *testing.T) {
	t.Helper()
	for _, e := range h.analytics() {
		p, ok := e["properties"].(map[string]any)
		if !ok {
			t.Fatal("missing properties")
		}
		if p["$process_person_profile"] != false || p["$geoip_disable"] != true {
			t.Fatal("person or GeoIP processing enabled")
		}
		for k := range p {
			switch k {
			case "schema_version", "product", "product_version", "os", "arch", "result", "code", "content_kind", "backend", "action_category", "source_kind", "$process_person_profile", "$geoip_disable":
			default:
				t.Fatalf("unexpected SDK property %q", k)
			}
		}
	}
	h.mu.Lock()
	defer h.mu.Unlock()
	for _, r := range h.requests {
		if !r.Authorized {
			t.Fatalf("ingestion credential not delivered to %s", r.Path)
		}
		for _, canary := range []string{"otlp-env-secret", "fixture-private", "posthog-private-clipboard-canary", h.home, "phc_unic_e2e_only"} {
			if strings.Contains(string(r.Body), canary) {
				t.Fatalf("private value in %s", r.Path)
			}
		}
	}
}
func (h *observabilityRun) deltaCount(t *testing.T, mark time.Time) int64 {
	t.Helper()
	h.mu.Lock()
	defer h.mu.Unlock()
	var total int64
	for _, r := range h.requests {
		if r.Path != "/i/v1/metrics" || r.At.Before(mark) {
			continue
		}
		var data collectmetrics.ExportMetricsServiceRequest
		if proto.Unmarshal(r.Body, &data) != nil {
			t.Fatal("invalid OTLP")
		}
		for _, rs := range data.ResourceMetrics {
			for _, scope := range rs.ScopeMetrics {
				for _, m := range scope.Metrics {
					if m.Name != "unic.pipeline.operations" {
						continue
					}
					sum := m.GetSum()
					if sum.GetAggregationTemporality() != metricspb.AggregationTemporality_AGGREGATION_TEMPORALITY_DELTA {
						t.Fatal("not delta")
					}
					for _, p := range sum.DataPoints {
						stage := ""
						for _, a := range p.Attributes {
							if a.Key == "stage" {
								stage = a.Value.GetStringValue()
							}
						}
						if stage == "commit" {
							total += p.GetAsInt()
						}
					}
				}
			}
		}
	}
	return total
}

func (h *observabilityRun) histogramCount(t *testing.T) uint64 {
	t.Helper()
	h.mu.Lock()
	defer h.mu.Unlock()
	var count uint64
	var sum float64
	for _, r := range h.requests {
		if r.Path != "/i/v1/metrics" {
			continue
		}
		var data collectmetrics.ExportMetricsServiceRequest
		if proto.Unmarshal(r.Body, &data) != nil {
			t.Fatal("invalid OTLP")
		}
		for _, rs := range data.ResourceMetrics {
			for _, scope := range rs.ScopeMetrics {
				for _, m := range scope.Metrics {
					if m.Name != "unic.pipeline.duration" {
						continue
					}
					hist := m.GetHistogram()
					if hist.GetAggregationTemporality() != metricspb.AggregationTemporality_AGGREGATION_TEMPORALITY_DELTA {
						t.Fatal("histogram not delta")
					}
					for _, p := range hist.DataPoints {
						fixture := false
						for _, a := range p.Attributes {
							if a.Key == "stage" && a.Value.GetStringValue() == "receive" {
								fixture = true
							}
						}
						if !fixture {
							continue
						}
						count += p.Count
						sum += p.GetSum()
						var buckets uint64
						for _, n := range p.BucketCounts {
							buckets += n
						}
						if buckets != p.Count {
							t.Fatal("histogram bucket count differs")
						}
					}
				}
			}
		}
	}
	if count == 3 && math.Abs(sum-.61) > .000001 {
		t.Fatalf("histogram sum=%f", sum)
	}
	return count
}

func TestPostHogPairingAndSend(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	a, b := newObservabilityRun(t), newObservabilityRun(t)
	a.posthog, b.posthog = true, true
	a.artifactName = "posthog-pair-alpha"
	b.artifactName = "posthog-pair-beta"
	for _, h := range []*observabilityRun{a, b} {
		h.startConfig(t, fmt.Sprintf("[telemetry]\nendpoint=%q\n", h.server.URL+"/i"))
	}
	a.run(t, "pair-new-trust", func(t *testing.T) {
		code, body := a.request(t, "POST", "/v1/pairing-ticket", "")
		if code != 200 {
			t.Fatal("ticket failed")
		}
		var ticket struct {
			Ticket string `json:"ticket"`
		}
		if json.Unmarshal(body, &ticket) != nil {
			t.Fatal("invalid ticket")
		}
		b.cli(t, "pair", ticket.Ticket)
		for _, h := range []*observabilityRun{a, b} {
			var pending []struct {
				ID          string `json:"id"`
				Fingerprint string `json:"fingerprint"`
			}
			h.eventually(t, func() bool { _ = json.Unmarshal(h.get(t, "/v1/pending"), &pending); return len(pending) > 0 })
			h.cli(t, "approve", pending[0].ID, pending[0].Fingerprint)
			h.eventually(t, func() bool { return h.eventCount("pairing_completed") == 1 })
		}
	})
	a.run(t, "manual-send-terminal-and-automatic-exclusion", func(t *testing.T) {
		for _, h := range []*observabilityRun{a, b} {
			h.eventually(t, func() bool { return strings.Contains(string(h.get(t, "/v1/status")), `"connected":true`) })
		}
		a.cli(t, "copy", "posthog-private-clipboard-canary")
		b.eventually(t, func() bool {
			code, body := b.request(t, "GET", "/v1/clipboard", "")
			return code == 200 && strings.Contains(string(body), "posthog-private-clipboard-canary")
		})
		a.eventually(t, func() bool { return a.eventCount("manual_send_completed") == 1 })
		for _, ev := range a.analytics() {
			if ev["event"] == "manual_send_completed" && ev["properties"].(map[string]any)["result"] != "succeeded" {
				t.Fatal("send terminal result is wrong")
			}
		}
		// Automatic received application is not an explicit history application or send.
		time.Sleep(300 * time.Millisecond)
		if b.eventCount("history_applied") != 0 || b.eventCount("manual_send_completed") != 0 {
			t.Fatal("background receive emitted a user action")
		}
		for _, h := range []*observabilityRun{a, b} {
			h.assertPostHogPrivacy(t)
		}
	})
}
func (h *observabilityRun) eventCount(name string) int {
	count := 0
	for _, ev := range h.analytics() {
		if ev["event"] == name {
			count++
		}
	}
	return count
}

func TestPostHogResources(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	h := newObservabilityRun(t)
	h.posthog = true
	h.artifactName = "posthog-resources"
	h.startConfig(t, fmt.Sprintf("[telemetry]\nendpoint=%q\n[telemetry.logs]\nenabled=false\n[telemetry.metrics]\nenabled=false\n", h.server.URL+"/i"))
	h.run(t, "bounded-queue-and-loss", func(t *testing.T) {
		h.mu.Lock()
		h.responseDelay = 10 * time.Second
		h.mu.Unlock()
		code, _ := h.request(t, "POST", "/test/analytics-pressure", "")
		if code != 200 {
			t.Fatal("pressure fixture failed")
		}
		var state struct {
			Analytics struct {
				Pending int64  `json:"pending_records"`
				Dropped uint64 `json:"dropped_records"`
			} `json:"analytics"`
		}
		_ = json.Unmarshal(h.get(t, "/v1/diagnostics/telemetry"), &state)
		if state.Analytics.Pending > 1024 || state.Analytics.Dropped == 0 {
			t.Fatalf("queue bound/loss not proven: %+v", state.Analytics)
		}
		h.resourceSamples = append(h.resourceSamples, map[string]any{"phase": "saturated", "pending_records": state.Analytics.Pending, "dropped_records": state.Analytics.Dropped, "at": time.Now()})
		h.cli(t, "config", "set", "telemetry.enabled", "false")
		h.requireNoRequests(t, 300*time.Millisecond)
		h.mu.Lock()
		h.responseDelay = 0
		h.mu.Unlock()
	})
	h.run(t, "repeated-generation-cleanup", func(t *testing.T) {
		var before, after map[string]uint64
		_ = json.Unmarshal(h.get(t, "/test/resources"), &before)
		for i := 0; i < 20; i++ {
			h.cli(t, "config", "set", "telemetry.enabled", "true")
			h.cli(t, "config", "set", "telemetry.enabled", "false")
		}
		_ = json.Unmarshal(h.get(t, "/test/resources"), &after)
		if after["goroutines"] > before["goroutines"]+5 {
			t.Fatalf("workers leaked: %d -> %d", before["goroutines"], after["goroutines"])
		}
		h.resourceSamples = append(h.resourceSamples, map[string]any{"phase": "20-generations", "before": before, "after": after})
	})
}

func TestPostHogTransport(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	for _, scenario := range []struct {
		name      string
		status    int
		untrusted bool
		delay     time.Duration
	}{
		{"unauthorized", 401, false, 0}, {"unavailable", 503, false, 0}, {"untrusted-tls", 200, true, 0}, {"timeout", 200, false, 10 * time.Second}, {"redirect", 302, false, 0},
	} {
		t.Run(scenario.name, func(t *testing.T) {
			h := newObservabilityRun(t)
			h.posthog = true
			h.artifactName = "posthog-transport-" + scenario.name
			h.status = scenario.status
			h.untrusted = scenario.untrusted
			h.responseDelay = scenario.delay
			if scenario.name == "redirect" {
				h.responseLocation = "https://127.0.0.1:1/credential-canary"
			}
			h.startConfig(t, fmt.Sprintf("[telemetry]\nendpoint=%q\n[telemetry.logs]\nenabled=false\n[telemetry.metrics]\nenabled=false\n", h.server.URL+"/i"))
			h.eventually(t, func() bool {
				var state struct {
					Analytics struct {
						State    string `json:"state"`
						Failures uint64 `json:"export_failures"`
					} `json:"analytics"`
				}
				_ = json.Unmarshal(h.get(t, "/v1/diagnostics/telemetry"), &state)
				return state.Analytics.State == "degraded" && state.Analytics.Failures > 0
			})
			h.mu.Lock()
			before := len(h.requests)
			h.mu.Unlock()
			h.requireNoRequests(t, 400*time.Millisecond)
			h.cli(t, "config", "set", "telemetry.enabled", "false")
			h.requireNoRequests(t, 200*time.Millisecond)
			h.assertions = append(h.assertions, map[string]any{"scenario": scenario.name, "state": "degraded", "request_count": before, "retries": 0})
		})
	}
}

// A production build proves release metadata assembly independently of the
// build-tagged token/interval fixtures. Production 60s/5s intervals are used.
func TestPostHogReleaseAssembly(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	h := newObservabilityRun(t)
	h.posthog = true
	h.production = true
	h.artifactName = "posthog-release"
	h.executable = filepath.Join(h.dir, "unicd-release")
	root, err := repoRoot()
	if err != nil {
		t.Fatal(err)
	}
	flags := "-X github.com/uniclipboard/unic/internal/observability.PostHogHost=" + h.server.URL + " -X github.com/uniclipboard/unic/internal/observability.PostHogToken=phc_unic_e2e_only -X github.com/uniclipboard/unic/internal/observability.ProductVersion=acceptance"
	build := exec.Command("go", "build", "-race", "-ldflags", flags, "-o", h.executable, "./app/service/cmd/unicd")
	build.Dir = root
	if data, err := build.CombinedOutput(); err != nil {
		t.Fatalf("production build: %v %s", err, data)
	}
	h.run(t, "release-false-before-start", func(t *testing.T) {
		h.startConfig(t, "[telemetry]\nenabled=false\n")
		h.requireNoRequests(t, 600*time.Millisecond)
		h.stop(t)
	})
	h.run(t, "release-defaults-and-production-intervals", func(t *testing.T) {
		started := time.Now()
		h.startConfig(t, "")
		h.cli(t, "submit", "posthog-private-clipboard-canary", "--request-id", "posthog-release-submission")
		time.Sleep(200 * time.Millisecond)
		if err := os.WriteFile(filepath.Join(h.home, "config.toml"), []byte("[telemetry]\nenabled=\"invalid\"\n"), 0600); err != nil {
			t.Fatal(err)
		}
		h.eventually(t, func() bool {
			return h.hasPath("/i/v1/logs") && h.hasPath("/i/v1/metrics") && h.eventCount("app_started") == 1
		})
		if time.Since(started) < 55*time.Second {
			t.Fatal("production metric interval was shortened")
		}
		for _, ev := range h.analytics() {
			if ev["properties"].(map[string]any)["product_version"] != "acceptance" {
				t.Fatal("release version injection failed")
			}
		}
		h.assertPostHogPrivacy(t)
		h.stop(t)
	})
	h.run(t, "release-token-bound-to-endpoint", func(t *testing.T) {
		other := newObservabilityRun(t)
		other.artifactName = "posthog-release-other-endpoint"
		h.startConfig(t, fmt.Sprintf("[telemetry]\nendpoint=%q\n", other.server.URL+"/i"))
		h.contains(t, "/v1/diagnostics/telemetry", `"last_error_code":"not_configured"`)
		other.requireNoRequests(t, 500*time.Millisecond)
		h.requireNoRequests(t, 300*time.Millisecond)
	})
}

func TestPostHogEnableFromDisabled(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	h := newObservabilityRun(t)
	h.posthog = true
	h.artifactName = "posthog-enable-from-disabled"
	h.run(t, "first-enable-without-startup-replay", func(t *testing.T) {
		h.startConfig(t, fmt.Sprintf("[telemetry]\nenabled=false\nendpoint=%q\n", h.server.URL+"/i"))
		h.requireNoRequests(t, 300*time.Millisecond)
		if _, err := os.Stat(filepath.Join(h.home, "analytics-id")); !os.IsNotExist(err) {
			t.Fatal("disabled first startup created identity")
		}
		h.cli(t, "config", "set", "telemetry.enabled", "true")
		h.cli(t, "submit", "posthog-private-clipboard-canary", "--request-id", "posthog-after-enable")
		h.eventually(t, func() bool { return h.eventCount("manual_send_completed") == 1 })
		for _, ev := range h.analytics() {
			if ev["event"] == "app_started" && ev["properties"].(map[string]any)["product"] == "cli" {
				t.Fatal("old service startup was replayed")
			}
		}
		h.assertPostHogPrivacy(t)
	})
}

func TestPostHogDesktopReconnect(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	h := newObservabilityRun(t)
	h.posthog = true
	h.manualAdapter = true
	h.artifactName = "posthog-desktop-reconnect"
	h.startConfig(t, fmt.Sprintf("[telemetry]\nendpoint=%q\n", h.server.URL+"/i"))
	call := func(session, method string, value any) json.RawMessage {
		data, err := json.Marshal(map[string]any{"session": session, "method": method, "value": value})
		if err != nil {
			t.Fatal(err)
		}
		code, b := h.request(t, "POST", "/v1/adapter", string(data))
		if code != 200 {
			t.Fatal("adapter HTTP failed")
		}
		var reply struct {
			Value json.RawMessage `json:"value"`
			Error any             `json:"error"`
		}
		if json.Unmarshal(b, &reply) != nil || reply.Error != nil {
			t.Fatal("adapter protocol failed")
		}
		return reply.Value
	}
	desktopCount := func() int {
		n := 0
		for _, e := range h.analytics() {
			if e["event"] == "app_started" && e["properties"].(map[string]any)["product"] == "desktop" {
				n++
			}
		}
		return n
	}
	h.run(t, "same-process-reconnect-is-not-startup", func(t *testing.T) {
		for i, key := range []string{"AAAAAAAAAAAAAAAA", "AAAAAAAAAAAAAAAA", "BBBBBBBBBBBBBBBB", ""} {
			var session string
			if json.Unmarshal(call("", "register", map[string]string{"launch_id": key}), &session) != nil {
				t.Fatal("invalid adapter session")
			}
			call(session, "capabilities", map[string]any{"Observe": "available", "Apply": "available"})
			call(session, "observe", map[string]any{"Epoch": fmt.Sprintf("reconnect-%d", i), "Sequence": 1, "Mode": "baseline"})
			call(session, "ready", nil)
			call(session, "ready", nil)
			expected := 1
			if i >= 2 {
				expected = 2
			}
			h.eventually(t, func() bool { return desktopCount() == expected })
			time.Sleep(150 * time.Millisecond)
			if desktopCount() != expected {
				t.Fatal("duplicate ready/reconnect counted as a startup")
			}
			call(session, "close", nil)
		}
		h.assertPostHogPrivacy(t)
		h.mu.Lock()
		defer h.mu.Unlock()
		for _, r := range h.requests {
			if strings.Contains(string(r.Body), "AAAAAAAAAAAAAAAA") || strings.Contains(string(r.Body), "BBBBBBBBBBBBBBBB") {
				t.Fatal("local launch ID was uploaded")
			}
		}
	})
}

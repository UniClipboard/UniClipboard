package e2e

import (
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
	"time"

	collectmetrics "go.opentelemetry.io/proto/otlp/collector/metrics/v1"
	"google.golang.org/protobuf/proto"
)

// Defined before runtime instrumentation. Real services exercise identity, session,
// wire, Engine, host application reports and TLS export. Fake path controls prove
// plumbing only. Real tailcat direct/DERP and hosted acceptance remain separate.
func TestSyncObservability(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	a, b := newObservabilityRun(t), newObservabilityRun(t)
	a.artifactName, b.artifactName = "sync-alpha", "sync-beta"
	for _, h := range []*observabilityRun{a, b} {
		h.posthog = true
		h.startConfig(t, fmt.Sprintf("[telemetry]\nendpoint=%q\n", h.server.URL))
	}
	a.run(t, "connection-and-permission", func(t *testing.T) {
		pairSyncServices(t, a, b)
		for _, h := range []*observabilityRun{a, b} {
			h.eventually(t, func() bool {
				return h.syncMetric("unic.sync.connection.attempts", map[string]string{"phase": "identity_handshake", "result": "succeeded"}) > 0
			})
		}
		if a.syncMetric("unic.sync.connection.attempts", map[string]string{"phase": "network_start", "result": "succeeded"}) != 1 {
			t.Fatal("network start terminal count")
		}
	})
	a.run(t, "payload-and-remote-daily", func(t *testing.T) {
		for i := 0; i < 2; i++ {
			text := fmt.Sprintf("0020-private-content-%d", i)
			a.cli(t, "copy", text)
			b.eventually(t, func() bool {
				code, body := b.request(t, "GET", "/v1/clipboard", "")
				return code == 200 && strings.Contains(string(body), text)
			})
		}
		size := float64(len("0020-private-content-0") * 2)
		a.eventually(t, func() bool {
			return a.syncMetric("unic.sync.transfer.bytes", map[string]string{"direction": "outgoing", "result": "succeeded"}) == size
		})
		b.eventually(t, func() bool {
			return b.syncMetric("unic.sync.transfer.bytes", map[string]string{"direction": "incoming", "result": "succeeded"}) == size && b.eventCount("remote_content_applied") == 1
		})
		b.eventually(t, func() bool { return b.syncMetric("unic.sync.remote_applications", nil) == 2 })
		b.cli(t, "history", "use", "@1", "--request-id", "0020-remote-restore")
		b.cli(t, "history", "use", "@1", "--request-id", "0020-remote-restore")
		b.eventually(t, func() bool { return b.eventCount("history_applied") == 1 })
		for _, ev := range b.analytics() {
			if ev["event"] == "history_applied" && ev["properties"].(map[string]any)["source_kind"] != "remote" {
				t.Fatal("history source classification")
			}
		}
		if b.eventCount("remote_content_applied") != 1 {
			t.Fatal("manual restore emitted automatic event")
		}
	})
	a.run(t, "path-plumbing-and-recovery", func(t *testing.T) {
		for _, h := range []*observabilityRun{a, b} {
			syncControl(t, h, "path-derp")
		}
		// The production cadence is retained. No active path probes run in this fixture.
		a.eventually(t, func() bool { return a.syncMetric("unic.sync.path.samples", map[string]string{"path": "derp"}) > 0 })
		for _, h := range []*observabilityRun{a, b} {
			syncControl(t, h, "path-direct")
		}
		a.eventually(t, func() bool {
			return a.syncMetric("unic.sync.path.transitions", map[string]string{"from": "derp", "to": "direct"}) > 0
		})
		syncControl(t, a, "disconnect")
		for _, h := range []*observabilityRun{a, b} {
			h.eventually(t, func() bool {
				return h.syncMetric("unic.sync.recovery.ends", map[string]string{"result": "recovered"}) >= 1
			})
		}
		a.cli(t, "copy", "0020-after-recovery")
		b.eventually(t, func() bool {
			c, v := b.request(t, "GET", "/v1/clipboard", "")
			return c == 200 && strings.Contains(string(v), "0020-after-recovery")
		})
	})
	a.run(t, "disable-and-fresh-baseline", func(t *testing.T) {
		for _, h := range []*observabilityRun{a, b} {
			h.cli(t, "config", "set", "telemetry.metrics.enabled", "false")
		}
		a.cli(t, "copy", "0020-disabled-payload")
		b.eventually(t, func() bool {
			c, v := b.request(t, "GET", "/v1/clipboard", "")
			return c == 200 && strings.Contains(string(v), "0020-disabled-payload")
		})
		for _, h := range []*observabilityRun{a, b} {
			h.mu.Lock()
			h.syncMetricStart = time.Now()
			h.mu.Unlock()
			h.cli(t, "config", "set", "telemetry.metrics.enabled", "true")
		}
		time.Sleep(1100 * time.Millisecond)
		if a.syncMetric("unic.sync.transfer.bytes", nil) != 0 {
			t.Fatal("disabled payload replayed")
		}
	})
	a.run(t, "daily-marker-restart-and-rollover", func(t *testing.T) {
		b.stop(t)
		if err := os.WriteFile(filepath.Join(b.home, ".analytics-remote-day.tmp"), []byte("private-stale-daily-marker"), 0600); err != nil {
			t.Fatal(err)
		}
		b.startConfig(t, fmt.Sprintf("[telemetry]\nendpoint=%q\n", b.server.URL))
		if _, err := os.Lstat(filepath.Join(b.home, ".analytics-remote-day.tmp")); !os.IsNotExist(err) {
			t.Fatal("stale daily temporary retained")
		}

		a.eventually(t, func() bool { return strings.Contains(string(a.get(t, "/v1/status")), `"connected":true`) })
		a.cli(t, "copy", "0020-restart-same-day")
		b.eventually(t, func() bool {
			c, v := b.request(t, "GET", "/v1/clipboard", "")
			return c == 200 && strings.Contains(string(v), "0020-restart-same-day")
		})
		time.Sleep(500 * time.Millisecond)
		if b.eventCount("remote_content_applied") != 1 {
			t.Fatal("same UTC day admitted after restart")
		}
		syncControl(t, b, "next-day")
		a.cli(t, "copy", "0020-next-utc-day")
		b.eventually(t, func() bool { return b.eventCount("remote_content_applied") == 2 })
	})

	a.run(t, "analytics-disable-and-identity-rotation", func(t *testing.T) {
		previous := b.analytics()[len(b.analytics())-1]["distinct_id"]
		b.cli(t, "config", "set", "telemetry.analytics.enabled", "false")
		for _, name := range []string{"analytics-id", "analytics-remote-day", ".analytics-remote-day.tmp"} {
			if _, err := os.Lstat(filepath.Join(b.home, name)); !os.IsNotExist(err) {
				t.Fatal("analytics private state retained on disable")
			}
		}
		a.cli(t, "copy", "0020-analytics-disabled")
		b.eventually(t, func() bool {
			code, body := b.request(t, "GET", "/v1/clipboard", "")
			return code == 200 && strings.Contains(string(body), "0020-analytics-disabled")
		})
		b.eventually(t, func() bool { return b.syncMetric("unic.sync.remote_applications", nil) == 3 })
		b.cli(t, "config", "set", "telemetry.analytics.enabled", "true")
		time.Sleep(250 * time.Millisecond)
		if b.eventCount("remote_content_applied") != 2 {
			t.Fatal("disabled-period application replayed")
		}
		a.cli(t, "copy", "0020-analytics-new-identity")
		b.eventually(t, func() bool { return b.eventCount("remote_content_applied") == 3 })
		events := b.analytics()
		if events[len(events)-1]["distinct_id"] == previous {
			t.Fatal("daily marker reused old analytics identity")
		}
	})
	a.run(t, "privacy", func(t *testing.T) {
		for _, h := range []*observabilityRun{a, b} {
			h.assertSyncPrivacy(t)
			h.assertOTLP(t)
		}
	})
}

func syncControl(t *testing.T, h *observabilityRun, op string) {
	t.Helper()
	code, _ := h.request(t, "POST", "/test/sync/"+op, "")
	if code != 200 {
		t.Fatalf("sync fixture %s: %d", op, code)
	}
}
func pairSyncServices(t *testing.T, a, b *observabilityRun) {
	t.Helper()
	code, body := a.request(t, "POST", "/v1/pairing-ticket", "")
	if code != 200 {
		t.Fatal("pairing ticket")
	}
	var ticket struct {
		Ticket string `json:"ticket"`
	}
	if json.Unmarshal(body, &ticket) != nil {
		t.Fatal("ticket decode")
	}
	b.cli(t, "pair", ticket.Ticket)
	for _, h := range []*observabilityRun{a, b} {
		var pending []struct {
			ID          string `json:"id"`
			Fingerprint string `json:"fingerprint"`
		}
		h.eventually(t, func() bool { _ = json.Unmarshal(h.get(t, "/v1/pending"), &pending); return len(pending) > 0 })
		h.cli(t, "approve", pending[0].ID, pending[0].Fingerprint)
	}
	for _, h := range []*observabilityRun{a, b} {
		h.eventually(t, func() bool { return strings.Contains(string(h.get(t, "/v1/status")), `"connected":true`) })
	}
}

// Pathless local receiver uses cumulative aggregation. Keep the latest value
// per label set, then sum matching sets. Never sum repeated cumulative exports.
func (h *observabilityRun) syncMetric(name string, want map[string]string) float64 {
	h.mu.Lock()
	defer h.mu.Unlock()
	latest := map[string]float64{}
	for _, r := range h.requests {
		if r.At.Before(h.syncMetricStart) || !strings.HasSuffix(r.Path, "/v1/metrics") {
			continue
		}
		var req collectmetrics.ExportMetricsServiceRequest
		if proto.Unmarshal(r.Body, &req) != nil {
			continue
		}
		for _, rm := range req.ResourceMetrics {
			for _, sm := range rm.ScopeMetrics {
				for _, m := range sm.Metrics {
					if m.Name != name {
						continue
					}
					for _, p := range m.GetSum().GetDataPoints() {
						attrs := map[string]string{}
						for _, kv := range p.Attributes {
							attrs[kv.Key] = kv.Value.GetStringValue()
						}
						match := true
						for k, v := range want {
							if attrs[k] != v {
								match = false
							}
						}
						if match {
							k, _ := json.Marshal(attrs)
							v := float64(p.GetAsInt())
							if p.GetAsDouble() != 0 {
								v = p.GetAsDouble()
							}
							latest[string(k)] = v
						}
					}
				}
			}
		}
	}
	n := float64(0)
	for _, v := range latest {
		n += v
	}
	return n
}
func (h *observabilityRun) assertSyncPrivacy(t *testing.T) {
	t.Helper()
	h.mu.Lock()
	defer h.mu.Unlock()
	for _, r := range h.requests {
		b := r.Body
		if strings.HasSuffix(r.Path, "/v1/metrics") {
			var req collectmetrics.ExportMetricsServiceRequest
			if proto.Unmarshal(b, &req) != nil {
				t.Fatal("OTLP decode")
			}
			b, _ = json.Marshal(&req)
		}
		for _, s := range []string{"0020-", h.home, "127.0.0.1"} {
			if strings.Contains(string(b), s) {
				t.Fatal("private value exported")
			}
		}
	}
}

func (h *observabilityRun) syncArtifacts(dir string, decoded []map[string]any) {
	write := func(name string, v any) {
		b, _ := json.MarshalIndent(v, "", "  ")
		_ = os.WriteFile(filepath.Join(dir, name), b, 0600)
	}
	write("scenarios.json", map[string]any{"executed": h.scenarioResults, "assertions": h.assertions, "real_tailcat_direct": "NOT_RUN", "real_tailcat_derp": "NOT_RUN", "symmetric_client_status": "BLOCKED_UPSTREAM", "hosted": "NOT_RUN", "native_clipboard": "NOT_RUN"})
	write("network-conditions.json", map[string]any{"transport": "real session/wire over local TCP", "path_source": "build-tagged finite fake; export plumbing only", "active_probes": 0, "production_sampling_seconds": 10, "real_stack": "NOT_RUN"})
	write("resource-summary.json", map[string]any{"samples": h.resourceSamples, "threshold_acceptance": "NOT_RUN"})
	var otlp, analytics, timeline strings.Builder
	for _, r := range decoded {
		line, _ := json.Marshal(r)
		if r["path"] == "/batch/" {
			fmt.Fprintln(&analytics, string(line))
		} else {
			fmt.Fprintln(&otlp, string(line))
		}
	}
	for _, r := range h.requests {
		line, _ := json.Marshal(map[string]any{"at": r.At, "path": r.Path, "authorized": r.Authorized})
		fmt.Fprintln(&timeline, string(line))
	}
	_ = os.WriteFile(filepath.Join(dir, "otlp.decoded.jsonl"), []byte(otlp.String()), 0600)
	_ = os.WriteFile(filepath.Join(dir, "analytics.decoded.jsonl"), []byte(analytics.String()), 0600)
	_ = os.WriteFile(filepath.Join(dir, "timeline.jsonl"), []byte(timeline.String()), 0600)
}

// Failure controls act only on the build-tagged TCP transport. Every assertion
// still observes production session/wire/Engine decisions through decoded OTLP.
func TestSyncObservabilityFaults(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	a, b := newObservabilityRun(t), newObservabilityRun(t)
	a.artifactName, b.artifactName = "sync-fault-alpha", "sync-fault-beta"
	for _, h := range []*observabilityRun{a, b} {
		h.posthog = true
		h.startConfig(t, fmt.Sprintf("[telemetry]\nendpoint=%q\n", h.server.URL))
	}
	pairSyncServices(t, a, b)
	a.run(t, "partial-body", func(t *testing.T) {
		syncControl(t, a, "partial-body")
		a.cli(t, "copy", strings.Repeat("0020-partial-canary", 1024))
		a.eventually(t, func() bool {
			return a.syncMetric("unic.sync.transfer.bytes", map[string]string{"direction": "outgoing", "result": "failed"}) == 17
		})
		b.eventually(t, func() bool {
			return b.syncMetric("unic.sync.transfer.bytes", map[string]string{"direction": "incoming", "result": "failed"}) == 17
		})
		if b.eventCount("remote_content_applied") != 0 {
			t.Fatal("partial body became clipboard availability")
		}
	})
	a.run(t, "pending-recovery-and-timeout", func(t *testing.T) {
		for _, h := range []*observabilityRun{a, b} {
			h.eventually(t, func() bool { return strings.Contains(string(h.get(t, "/v1/status")), `"connected":true`) })
		}
		for _, h := range []*observabilityRun{a, b} {
			syncControl(t, h, "dial-timeout")
		}
		before := a.syncMetric("unic.sync.recovery.ends", map[string]string{"result": "recovered"})
		syncControl(t, a, "disconnect")
		a.eventually(t, func() bool {
			return a.syncMetric("unic.sync.connection.errors", map[string]string{"phase": "dial", "reason": "timeout"}) >= 2
		})
		a.eventually(t, func() bool { return a.syncHistogramCount("unic.sync.recovery.pending", nil) > 0 })
		if a.syncMetric("unic.sync.recovery.ends", map[string]string{"result": "recovered"}) != before {
			t.Fatal("pending outage reported as recovered")
		}
		for _, h := range []*observabilityRun{a, b} {
			syncControl(t, h, "dial-normal")
		}
		for _, h := range []*observabilityRun{a, b} {
			h.eventually(t, func() bool { return strings.Contains(string(h.get(t, "/v1/status")), `"connected":true`) })
		}
		a.eventually(t, func() bool {
			return a.syncMetric("unic.sync.recovery.ends", map[string]string{"result": "recovered"}) == before+1
		})
	})
	a.run(t, "daily-persistence-failure", func(t *testing.T) {
		// Refuse a nonregular marker on restart; other analytics remain usable.
		b.stop(t)
		if err := os.Mkdir(filepath.Join(b.home, "analytics-remote-day"), 0700); err != nil {
			t.Fatal(err)
		}
		b.startConfig(t, fmt.Sprintf("[telemetry]\nendpoint=%q\n", b.server.URL))
		a.eventually(t, func() bool { return strings.Contains(string(a.get(t, "/v1/status")), `"connected":true`) })
		a.cli(t, "copy", "0020-marker-failure")
		b.eventually(t, func() bool {
			code, body := b.request(t, "GET", "/v1/clipboard", "")
			return code == 200 && strings.Contains(string(body), "0020-marker-failure")
		})
		b.eventually(t, func() bool { return b.syncMetric("unic.sync.remote_applications", nil) > 0 })
		if b.eventCount("remote_content_applied") != 0 {
			t.Fatal("failed marker admitted daily event")
		}
		b.assertSyncPrivacy(t)
	})
	a.run(t, "local-stop-is-expected", func(t *testing.T) {
		a.stop(t)
		a.eventually(t, func() bool {
			return a.syncMetric("unic.sync.session.ends", map[string]string{"reason": "local_stop", "expected": "true"}) > 0
		})
	})

}

func (h *observabilityRun) syncHistogramCount(name string, want map[string]string) uint64 {
	h.mu.Lock()
	defer h.mu.Unlock()
	latest := map[string]uint64{}
	for _, r := range h.requests {
		if r.At.Before(h.syncMetricStart) || !strings.HasSuffix(r.Path, "/v1/metrics") {
			continue
		}
		var req collectmetrics.ExportMetricsServiceRequest
		if proto.Unmarshal(r.Body, &req) != nil {
			continue
		}
		for _, rm := range req.ResourceMetrics {
			for _, sm := range rm.ScopeMetrics {
				for _, m := range sm.Metrics {
					if m.Name != name {
						continue
					}
					for _, p := range m.GetHistogram().GetDataPoints() {
						attrs := map[string]string{}
						for _, kv := range p.Attributes {
							attrs[kv.Key] = kv.Value.GetStringValue()
						}
						match := true
						for k, v := range want {
							if attrs[k] != v {
								match = false
							}
						}
						if match {
							key, _ := json.Marshal(attrs)
							latest[string(key)] = p.Count
						}
					}
				}
			}
		}
	}
	var n uint64
	for _, v := range latest {
		n += v
	}
	return n
}

// Resource limits are fixed before this measurement. Baseline is built from
// edad4381, with the same race build and normal export cadence. These limits
// are a local regression check, not a retroactive claim of preimplementation P0.
func TestSyncObservabilityResources(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	baseline := os.Getenv("UNIC_SYNC_BASELINE_DAEMON")
	if baseline == "" {
		t.Fatal("UNIC_SYNC_BASELINE_DAEMON must name the unchanged baseline binary")
	}
	if _, err := os.Stat(baseline); err != nil {
		t.Fatal(err)
	}
	for _, peers := range []int{0, 1, 8} {
		t.Run(fmt.Sprint(peers), func(t *testing.T) {
			samples := map[string]map[string]float64{}
			for _, variant := range []string{"baseline", "candidate"} {
				t.Run(variant, func(t *testing.T) {
					h := newObservabilityRun(t)
					h.artifactName = fmt.Sprintf("sync-resource-%d-%s", peers, variant)
					h.production = true
					if variant == "baseline" {
						h.executable = baseline
					}
					h.startConfig(t, fmt.Sprintf("[telemetry]\nendpoint=%q\n[telemetry.logs]\nenabled=false\n[telemetry.analytics]\nenabled=false\n", h.server.URL))
					for i := 0; i < peers; i++ {
						other := newObservabilityRun(t)
						other.artifactName = fmt.Sprintf("sync-resource-%d-%s-peer-%d", peers, variant, i)
						other.production = true
						if variant == "baseline" {
							other.executable = baseline
						}
						other.start(t, "")
						pairSyncServices(t, h, other)
					}
					time.Sleep(500 * time.Millisecond)
					before := processCPU(t, h.cmd.Process.Pid)
					time.Sleep(11 * time.Second)
					after := processCPU(t, h.cmd.Process.Pid)
					var mem map[string]float64
					if json.Unmarshal(h.get(t, "/test/resources"), &mem) != nil {
						t.Fatal("resource decode")
					}
					mem["cpu_seconds_11s"] = after - before
					mem["rss_kib"] = processRSS(t, h.cmd.Process.Pid)
					samples[variant] = mem
					h.resourceSamples = append(h.resourceSamples, map[string]any{"variant": variant, "eligible_sessions": peers, "goroutines": mem["goroutines"], "heap_alloc": mem["heap_alloc"], "rss_kib": mem["rss_kib"], "cpu_seconds_11s": mem["cpu_seconds_11s"]})
				})
			}
			limits := map[string]float64{"goroutines": 8, "heap_alloc": 8 << 20, "rss_kib": 16 << 10, "cpu_seconds_11s": .25}
			for key, limit := range limits {
				if samples["candidate"][key] > samples["baseline"][key]+limit {
					t.Errorf("%s: baseline %.3f, candidate %.3f, allowed increase %.3f", key, samples["baseline"][key], samples["candidate"][key], limit)
				}
			}
			dir := os.Getenv("UNIC_E2E_ARTIFACTS")
			if dir != "" {
				data, _ := json.MarshalIndent(map[string]any{"peers": peers, "measurements": samples, "maximum_increase": limits, "failed": t.Failed(), "baseline_head": "edad4381b8a080d583c7a558c517b2d671c8fdf3", "wakeups": "NOT_RUN", "collection_cost": "NOT_RUN"}, "", "  ")
				if err := os.WriteFile(filepath.Join(dir, fmt.Sprintf("resource-comparison-%d.json", peers)), data, 0600); err != nil {
					t.Fatal(err)
				}
			}
		})
	}
}
func processCPU(t *testing.T, pid int) float64 {
	t.Helper()
	out, err := exec.Command("ps", "-p", fmt.Sprint(pid), "-o", "time=").Output()
	if err != nil {
		t.Fatal(err)
	}
	s := strings.TrimSpace(string(out))
	parts := strings.Split(s, ":")
	var v float64
	for _, p := range parts {
		n, err := strconv.ParseFloat(p, 64)
		if err != nil {
			t.Fatal(err)
		}
		v = v*60 + n
	}
	return v
}
func processRSS(t *testing.T, pid int) float64 {
	t.Helper()
	out, err := exec.Command("ps", "-p", fmt.Sprint(pid), "-o", "rss=").Output()
	if err != nil {
		t.Fatal(err)
	}
	n, err := strconv.ParseFloat(strings.TrimSpace(string(out)), 64)
	if err != nil {
		t.Fatal(err)
	}
	return n
}

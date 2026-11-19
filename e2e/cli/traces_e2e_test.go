package e2e

import (
	"bytes"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"sort"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/uniclipboard/unic/internal/observability"

	collecttrace "go.opentelemetry.io/proto/otlp/collector/trace/v1"
	tracepb "go.opentelemetry.io/proto/otlp/trace/v1"
	"google.golang.org/protobuf/encoding/protojson"
	"google.golang.org/protobuf/proto"
)

// Failure inventory: docs/exec/observability-traces-failures.md.
// Production programs, no test tags, GUI or clipboard adapter.
func TestTraces(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	h := newObservabilityRun(t)
	h.production = true
	h.manualAdapter = true
	h.artifactName = "traces"
	h.executable = buildTraceDaemon(t, h, "1")
	h.cliExecutable = buildTraceCLI(t, h)
	h.requestHeaders = map[string]string{"traceparent": "00-11111111111111111111111111111111-2222222222222222-01", "tracestate": "private=trace-header-canary", "baggage": "private=trace-baggage-canary"}
	cfg := fmt.Sprintf("[sync]\nenabled=false\n[telemetry]\nendpoint=%q\n[telemetry.logs]\nenabled=false\n[telemetry.metrics]\nenabled=false\n[telemetry.analytics]\nenabled=false\n", h.server.URL)
	h.run(t, "default-disabled-full-window", func(t *testing.T) {
		h.startConfig(t, cfg)
		traceSubmit(t, h, "trace-default-canary")
		h.requireNoRequests(t, 5500*time.Millisecond)
	})
	h.run(t, "enabled-and-idempotent", func(t *testing.T) {
		h.cli(t, "config", "set", "telemetry.traces.enabled", "true")
		traceSubmit(t, h, "trace-request-canary")
		traceSubmit(t, h, "trace-request-canary")
		h.eventually(t, func() bool { return len(traceSpans(t, h)) >= 2 })
		time.Sleep(5500 * time.Millisecond)
		assertTraceTrees(t, h, 1)
	})
	h.run(t, "master-disabled-full-window", func(t *testing.T) {
		h.cli(t, "config", "set", "telemetry.enabled", "false")
		traceSubmit(t, h, "trace-master-off")
		h.requireNoRequests(t, 5500*time.Millisecond)
		h.cli(t, "config", "set", "telemetry.enabled", "true")
	})
	h.run(t, "signal-disabled-full-window", func(t *testing.T) {
		h.cli(t, "config", "set", "telemetry.traces.enabled", "false")
		traceSubmit(t, h, "trace-signal-off")
		h.requireNoRequests(t, 5500*time.Millisecond)
		h.cli(t, "config", "set", "telemetry.traces.enabled", "true")
	})
	for _, fault := range []struct {
		status int
		delay  time.Duration
		code   string
	}{{401, 0, "unauthorized"}, {429, 0, "rate_limited"}, {503, 0, "unavailable"}, {200, 4 * time.Second, "timeout"}} {
		h.run(t, "export-"+fault.code, func(t *testing.T) {
			h.mu.Lock()
			h.status = fault.status
			h.responseDelay = fault.delay
			h.mu.Unlock()
			beforeDrops := traceState(t, h).Dropped
			traceSubmit(t, h, "trace-fault-"+fault.code)
			h.eventually(t, func() bool { return traceState(t, h).Error != nil && *traceState(t, h).Error == fault.code })
			if traceState(t, h).Dropped < beforeDrops+2 {
				t.Fatal("failed export did not count discarded spans")
			}
			h.mu.Lock()
			h.status = 200
			h.responseDelay = 0
			h.mu.Unlock()
		})
	}
	h.run(t, "saturation-and-disable-generation", func(t *testing.T) {
		h.mu.Lock()
		h.responseDelay = 2500 * time.Millisecond
		h.mu.Unlock()
		resourcesBefore := traceProcessSample(t, h)
		before := traceState(t, h).Dropped
		for i := 0; i < 600; i++ {
			traceSubmit(t, h, fmt.Sprintf("pressure-%d", i))
		}
		h.resourceSamples = append(h.resourceSamples, map[string]any{"phase": "queue-pressure", "before": resourcesBefore, "after": traceProcessSample(t, h), "dropped_spans": traceState(t, h).Dropped - before})
		if traceState(t, h).Dropped <= before {
			t.Fatal("bounded queue did not report saturation")
		}
		began := time.Now()
		h.cli(t, "config", "set", "telemetry.traces.enabled", "false")
		if time.Since(began) > time.Second {
			t.Fatal("disable waited for slow exporter")
		}
		h.requireNoRequests(t, 5500*time.Millisecond)
		h.mu.Lock()
		h.traceSince = time.Now()
		h.responseDelay = 0
		h.mu.Unlock()
		h.cli(t, "config", "set", "telemetry.traces.enabled", "true")
		traceSubmit(t, h, "trace-fresh-generation")
		h.eventually(t, func() bool { return len(traceSpans(t, h)) >= 2 })
		time.Sleep(5500 * time.Millisecond)
		assertTraceTrees(t, h, 1)
	})
	h.run(t, "settings-survive-restart", func(t *testing.T) {
		h.stop(t)
		saved, err := os.ReadFile(filepath.Join(h.home, "config.toml"))
		if err != nil {
			t.Fatal(err)
		}
		h.mu.Lock()
		h.traceSince = time.Now()
		h.mu.Unlock()
		h.startConfig(t, string(saved))
		if traceState(t, h).State != "active" {
			t.Fatal("saved switch not active")
		}
		h.cli(t, "send", "--to", "all", "trace-content-canary", "--json")
		h.eventually(t, func() bool { return len(traceSpans(t, h)) >= 2 })
		assertTraceTrees(t, h, 1)
	})
	h.run(t, "switch-resource-release", func(t *testing.T) {
		before := traceProcessSample(t, h)
		for i := 0; i < 30; i++ {
			h.cli(t, "config", "set", "telemetry.traces.enabled", "false")
			h.cli(t, "config", "set", "telemetry.traces.enabled", "true")
		}
		time.Sleep(300 * time.Millisecond)
		after := traceProcessSample(t, h)
		h.resourceSamples = append(h.resourceSamples, map[string]any{"phase": "30-switch-cycles", "before": before, "after": after})
		if after.Handles > before.Handles+2 {
			t.Fatal("handles leaked across generations")
		}
	})
	h.run(t, "failed-import-privacy", func(t *testing.T) {
		h.cli(t, "config", "set", "telemetry.traces.enabled", "false")
		h.cli(t, "config", "set", "telemetry.traces.enabled", "true")
		h.traceSince = time.Now()
		body := `{"request_id":"trace-failed-canary","history_id":"trace-invalid-history-canary"}`
		code, _ := h.request(t, "POST", "/v1/submissions", body)
		if code < 400 {
			t.Fatal("invalid history accepted")
		}
		h.eventually(t, func() bool { return len(traceSpans(t, h)) > 0 })
		spans := traceSpans(t, h)
		if len(spans) != 1 || spans[0].Name != "unic.submission" || spans[0].Status.GetCode() != tracepb.Status_STATUS_CODE_ERROR {
			t.Fatal("failed import trace boundary")
		}
	})
	h.run(t, "shared-close-budget", func(t *testing.T) {
		h.cli(t, "config", "set", "telemetry.logs.enabled", "true")
		h.cli(t, "config", "set", "telemetry.metrics.enabled", "true")
		traceSubmit(t, h, "trace-shutdown")
		h.mu.Lock()
		h.responseDelay = 4 * time.Second
		h.mu.Unlock()
		began := time.Now()
		h.stop(t)
		if elapsed := time.Since(began); elapsed > 3*time.Second {
			t.Fatalf("shared close budget: %s", elapsed)
		}
	})

}

func buildTraceDaemon(t *testing.T, h *observabilityRun, ratio string) string {
	t.Helper()
	root, err := repoRoot()
	if err != nil {
		t.Fatal(err)
	}
	out := filepath.Join(h.dir, "unicd")
	args := []string{"build", "-o", out}
	if os.Getenv("UNIC_TRACE_RACE") == "1" {
		args = append(args, "-race")
	}
	if ratio != "" {
		args = append(args, "-ldflags", "-X github.com/uniclipboard/unic/internal/observability.traceSampleRatio="+ratio)
	}
	args = append(args, "./app/service/cmd/unicd")
	cmd := exec.Command("go", args...)
	cmd.Dir = root
	if b, e := cmd.CombinedOutput(); e != nil {
		t.Fatalf("production build: %v: %s", e, b)
	}
	return out
}
func traceSubmit(t *testing.T, h *observabilityRun, id string) {
	t.Helper()
	body, _ := json.Marshal(map[string]any{"request_id": id, "snapshot": map[string]any{"source_application": map[string]any{"platform": "darwin", "identifier": "trace-source-canary", "name": "trace-source-canary", "attribution": "foreground"}, "parts": []map[string]any{{"format": "text/plain", "data": []byte("trace-content-canary")}}}})
	code, b := h.request(t, "POST", "/v1/submissions", string(body))
	if code != 200 {
		t.Fatalf("submission failed: %d %s", code, b)
	}
}
func traceSpans(t *testing.T, h *observabilityRun) []*tracepb.Span {
	t.Helper()
	h.mu.Lock()
	defer h.mu.Unlock()
	var spans []*tracepb.Span
	for _, r := range h.requests {
		if r.Path != "/v1/traces" || r.At.Before(h.traceSince) {
			continue
		}
		var data collecttrace.ExportTraceServiceRequest
		if err := proto.Unmarshal(r.Body, &data); err != nil {
			t.Fatal(err)
		}
		decoded, _ := protojson.Marshal(&data)
		for _, secret := range []string{"trace-content-canary", "trace-request-canary", "trace-header-canary", "trace-baggage-canary", "trace-source-canary", "otlp-env-secret", h.dir} {
			if bytes.Contains(decoded, []byte(secret)) {
				t.Fatal("private canary in trace; payload omitted")
			}
		}
		for _, rs := range data.ResourceSpans {
			if len(rs.Resource.Attributes) != 4 {
				t.Fatal("unexpected trace resource")
			}
			for _, a := range rs.Resource.Attributes {
				if a.Key != "service.name" && a.Key != "service.version" && a.Key != "os.type" && a.Key != "host.arch" {
					t.Fatal("unexpected resource key")
				}
			}
			for _, ss := range rs.ScopeSpans {
				if len(ss.Scope.GetAttributes()) > 0 {
					t.Fatal("unexpected scope metadata")
				}
				for _, span := range ss.Spans {
					if bytes.Equal(span.TraceId, bytes.Repeat([]byte{0x11}, 16)) {
						t.Fatal("caller trace parent accepted")
					}
					for _, a := range span.Attributes {
						if a.Key != "result" && a.Key != "code" {
							t.Fatal("unexpected trace attribute")
						}
					}
				}
				spans = append(spans, ss.Spans...)
			}
		}
	}
	return spans
}
func assertTraceTrees(t *testing.T, h *observabilityRun, want int) {
	t.Helper()
	spans := traceSpans(t, h)
	roots := map[string]*tracepb.Span{}
	children := map[string][]*tracepb.Span{}
	for _, s := range spans {
		if len(s.Events) > 0 || len(s.Links) > 0 || s.TraceState != "" || s.Status.GetMessage() != "" {
			t.Fatal("unexpected span metadata")
		}
		if s.Name == "unic.submission" && len(s.ParentSpanId) == 0 {
			roots[string(s.TraceId)] = s
		} else if s.Name == "unic.commit" {
			children[string(s.TraceId)] = append(children[string(s.TraceId)], s)
		} else {
			t.Fatal("unexpected span name or parent")
		}
	}
	if len(roots) != want || len(spans) != want*2 {
		t.Fatalf("trees=%d spans=%d want=%d", len(roots), len(spans), want)
	}
	for id, r := range roots {
		cs := children[id]
		if len(cs) != 1 || !bytes.Equal(cs[0].ParentSpanId, r.SpanId) {
			t.Fatal("sampling/parent mismatch")
		}
	}
	h.assertions = append(h.assertions, map[string]any{"name": "trace trees", "roots": len(roots), "children": len(spans) - len(roots), "passed": true})
}

func traceState(t *testing.T, h *observabilityRun) observability.SignalState {
	t.Helper()
	var st observability.TelemetryState
	if err := json.Unmarshal(h.get(t, "/v1/diagnostics/telemetry"), &st); err != nil {
		t.Fatal(err)
	}
	return st.Traces
}

type traceProcessStats struct {
	CPUSeconds float64 `json:"cpu_seconds"`
	RSSKiB     int     `json:"rss_kib"`
	Handles    int     `json:"handles"`
}

func traceProcessSample(t *testing.T, h *observabilityRun) traceProcessStats {
	t.Helper()
	pid := strconv.Itoa(h.cmd.Process.Pid)
	out, err := exec.Command("ps", "-o", "time=,rss=", "-p", pid).Output()
	if err != nil {
		t.Fatal(err)
	}
	fields := strings.Fields(string(out))
	if len(fields) != 2 {
		t.Fatal("invalid process sample")
	}
	var cpu float64
	for _, v := range strings.Split(fields[0], ":") {
		n, e := strconv.ParseFloat(v, 64)
		if e != nil {
			t.Fatal(e)
		}
		cpu = cpu*60 + n
	}
	rss, err := strconv.Atoi(fields[1])
	if err != nil {
		t.Fatal(err)
	}
	fds, err := exec.Command("lsof", "-nP", "-a", "-p", pid, "-Ff").Output()
	if err != nil {
		t.Fatal(err)
	}
	handles := 0
	for _, v := range strings.Split(string(fds), "\n") {
		if strings.HasPrefix(v, "f") {
			if _, e := strconv.Atoi(v[1:]); e == nil {
				handles++
			}
		}
	}
	return traceProcessStats{cpu, rss, handles}
}

// Matched workloads use production daemons. Sampling overrides are link-time,
// never a runtime setting. The normal 1% build uses no sampling override.
func TestTracesCostAndSampling(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	if runtime.GOOS != "darwin" {
		t.Skip("process resource receipt currently targets macOS")
	}
	for _, mode := range []struct {
		name, ratio string
		enabled     bool
	}{{"disabled", "1", false}, {"unsampled", "0", true}, {"sampled", "1", true}, {"production", "", true}} {
		t.Run(mode.name, func(t *testing.T) {
			h := newObservabilityRun(t)
			h.production = true
			h.manualAdapter = true
			h.artifactName = "traces-cost-" + mode.name
			h.executable = buildTraceDaemon(t, h, mode.ratio)
			h.cliExecutable = buildTraceCLI(t, h)
			h.startConfig(t, fmt.Sprintf("[sync]\nenabled=false\n[telemetry]\nendpoint=%q\n[telemetry.logs]\nenabled=false\n[telemetry.metrics]\nenabled=false\n[telemetry.analytics]\nenabled=false\n[telemetry.traces]\nenabled=%t\n", h.server.URL, mode.enabled))
			before := traceProcessSample(t, h)
			began := time.Now()
			latencies := make([]int64, 1000)
			for i := range latencies {
				start := time.Now()
				traceSubmit(t, h, fmt.Sprintf("trace-load-%d", i))
				latencies[i] = time.Since(start).Microseconds()
			}
			time.Sleep(5500 * time.Millisecond)
			after := traceProcessSample(t, h)
			spans := traceSpans(t, h)
			roots := len(spans) / 2
			switch mode.name {
			case "disabled", "unsampled":
				if len(spans) != 0 {
					t.Fatal("unexpected exported spans")
				}
			case "sampled":
				assertTraceTrees(t, h, 1000)
			case "production":
				if roots < 1 || roots > 60 {
					t.Fatalf("unexpected 1%% root sample count: %d", roots)
				}
				assertTraceTrees(t, h, roots)
			}
			sort.Slice(latencies, func(i, j int) bool { return latencies[i] < latencies[j] })
			h.mu.Lock()
			requests := len(h.requests)
			bodyBytes := 0
			for _, r := range h.requests {
				bodyBytes += len(r.Body)
			}
			h.mu.Unlock()
			h.resourceSamples = append(h.resourceSamples, map[string]any{"mode": mode.name, "offered_roots": 1000, "exported_roots": roots, "before": before, "after": after, "elapsed_seconds": time.Since(began).Seconds(), "cpu_seconds": after.CPUSeconds - before.CPUSeconds, "p50_us": latencies[499], "p95_us": latencies[949], "p99_us": latencies[989], "http_requests": requests, "request_body_bytes": bodyBytes, "wire_bytes_received": h.wireReceived.Load(), "wire_bytes_sent": h.wireSent.Load(), "sync_latency": "NOT_RUN: independent trace workload uses sync disabled"})
		})
	}
}

func buildTraceCLI(t *testing.T, h *observabilityRun) string {
	t.Helper()
	out := filepath.Join(h.dir, "unic")
	root, e := repoRoot()
	if e != nil {
		t.Fatal(e)
	}
	cmd := exec.Command("go", "build", "-o", out, "./app/service/cmd/unic")
	cmd.Dir = root
	if b, e := cmd.CombinedOutput(); e != nil {
		t.Fatalf("production CLI build: %v: %s", e, b)
	}
	return out
}

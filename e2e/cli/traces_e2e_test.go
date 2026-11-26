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

	"github.com/uniclipboard/unic/internal/clip"
	"github.com/uniclipboard/unic/internal/observability"
	"github.com/uniclipboard/unic/internal/testboard"

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
	h.run(t, "default-enabled-and-idempotent", func(t *testing.T) {
		h.startConfig(t, cfg)
		if !traceState(t, h).Requested {
			t.Fatal("traces are not enabled by default")
		}
		traceSubmit(t, h, "trace-default-canary")
		traceSubmit(t, h, "trace-default-canary")
		h.eventually(t, func() bool { return len(traceSpans(t, h)) >= 2 })
		time.Sleep(5500 * time.Millisecond)
		assertTraceTrees(t, h, 1)
	})
	h.run(t, "enabled-and-idempotent", func(t *testing.T) {
		h.cli(t, "config", "set", "telemetry.traces.enabled", "false")
		h.traceSince = time.Now()
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
		h.stop(t)
		saved, err := os.ReadFile(filepath.Join(h.home, "config.toml"))
		if err != nil {
			t.Fatal(err)
		}
		h.startConfig(t, string(saved))
		if traceState(t, h).Requested {
			t.Fatal("explicit false was ignored after restart")
		}
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
	h.traceSamplingRatio = ratio
	if ratio == "" {
		h.traceSamplingRatio = "0.01"
	}
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
						if a.Key != "result" && a.Key != "code" && a.Key != "backend" && a.Key != "content_kind" && a.Key != "action_category" {
							t.Fatalf("unexpected trace attribute %q", a.Key)
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

func buildTraceE2EDaemon(t *testing.T, h *observabilityRun, ratio string) string {
	t.Helper()
	h.traceSamplingRatio = ratio
	root, err := repoRoot()
	if err != nil {
		t.Fatal(err)
	}
	out := filepath.Join(h.dir, "unicd-e2e")
	args := []string{"build", "-tags", "unic_e2e", "-o", out}
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
		t.Fatalf("e2e trace daemon build: %v: %s", e, b)
	}
	return out
}

func spanAttrMap(s *tracepb.Span) map[string]string {
	out := make(map[string]string, len(s.Attributes))
	for _, a := range s.Attributes {
		out[a.Key] = a.Value.GetStringValue()
	}
	return out
}

func TestTracesDistributedAndPipeline(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	a := newObservabilityRun(t)
	b := newObservabilityRun(t)
	a.artifactName = "traces-dist-alpha"
	b.artifactName = "traces-dist-beta"
	exe := buildTraceE2EDaemon(t, a, "1")
	cliExe := buildTraceCLI(t, a)
	a.executable, b.executable = exe, exe
	a.cliExecutable, b.cliExecutable = cliExe, cliExe

	cfgOf := func(url string) string {
		return fmt.Sprintf("[logging]\nlevel=\"info\"\n[telemetry]\nenabled=true\nendpoint=%q\n[telemetry.logs]\nenabled=false\n[telemetry.metrics]\nenabled=false\n[telemetry.analytics]\nenabled=false\n[telemetry.traces]\nenabled=true\n", url)
	}
	a.startConfig(t, cfgOf(a.server.URL))
	b.startConfig(t, cfgOf(b.server.URL))
	// Wait for the directory clipboard adapters to attach on both nodes.
	for _, h := range []*observabilityRun{a, b} {
		h.eventually(t, func() bool {
			var caps struct {
				Host struct {
					Observe string `json:"observe"`
					Apply   string `json:"apply"`
				} `json:"host"`
			}
			_ = json.Unmarshal(h.get(t, "/v1/capabilities"), &caps)
			return caps.Host.Observe == "available" && caps.Host.Apply == "available"
		})
	}
	pairSyncServices(t, a, b)

	// 1. Automatic clipboard capture on alpha -> cross-device propagation -> receive + apply on beta.
	a.mu.Lock()
	a.traceSince = time.Now()
	a.mu.Unlock()
	b.mu.Lock()
	b.traceSince = time.Now()
	b.mu.Unlock()

	if err := testboard.Dir(filepath.Join(a.home, "board")).Write(clip.Snapshot{
		Parts: []clip.Part{{Format: "text/plain", Data: []byte("trace-dist-auto-canary")}},
	}); err != nil {
		t.Fatalf("mock clipboard write: %v", err)
	}
	b.eventually(t, func() bool {
		got, err := testboard.Dir(filepath.Join(b.home, "board")).Read()
		if err != nil {
			return false
		}
		for _, p := range got.Parts {
			if p.Format == "text/plain" && string(p.Data) == "trace-dist-auto-canary" {
				return true
			}
		}
		return false
	})

	var obsSpan, commitSpan, sendSpan *tracepb.Span
	a.eventually(t, func() bool {
		obsSpan, commitSpan, sendSpan = nil, nil, nil
		for _, s := range traceSpans(t, a) {
			switch s.Name {
			case "unic.observe":
				obsSpan = s
			case "unic.commit":
				commitSpan = s
			case "unic.sync.send":
				sendSpan = s
			}
		}
		return obsSpan != nil && commitSpan != nil && sendSpan != nil
	})
	if len(obsSpan.ParentSpanId) != 0 || !bytes.Equal(commitSpan.ParentSpanId, obsSpan.SpanId) || !bytes.Equal(sendSpan.ParentSpanId, commitSpan.SpanId) {
		t.Fatalf("broken sender observe/commit/send hierarchy")
	}
	if !bytes.Equal(obsSpan.TraceId, commitSpan.TraceId) || !bytes.Equal(commitSpan.TraceId, sendSpan.TraceId) {
		t.Fatalf("sender spans do not share trace ID")
	}
	if sendSpan.Kind != tracepb.Span_SPAN_KIND_CLIENT {
		t.Fatalf("expected unic.sync.send kind=CLIENT, got %v", sendSpan.Kind)
	}
	if attrs := spanAttrMap(sendSpan); attrs["backend"] != "tailcat" || attrs["content_kind"] != "text" || attrs["result"] != "succeeded" {
		t.Fatalf("unexpected send attrs: %+v", attrs)
	}

	var recvSpan, applySpan *tracepb.Span
	b.eventually(t, func() bool {
		recvSpan, applySpan = nil, nil
		for _, s := range traceSpans(t, b) {
			if !bytes.Equal(s.TraceId, sendSpan.TraceId) {
				continue
			}
			switch s.Name {
			case "unic.sync.receive":
				recvSpan = s
			case "unic.apply":
				applySpan = s
			}
		}
		return recvSpan != nil && applySpan != nil
	})
	if recvSpan.Kind != tracepb.Span_SPAN_KIND_SERVER || !bytes.Equal(recvSpan.ParentSpanId, sendSpan.SpanId) {
		t.Fatalf("cross-device receive parent/kind mismatch: kind=%v", recvSpan.Kind)
	}
	if !bytes.Equal(applySpan.ParentSpanId, recvSpan.SpanId) {
		t.Fatalf("receiver apply span not parented to unic.sync.receive")
	}
	if attrs := spanAttrMap(recvSpan); attrs["backend"] != "tailcat" || attrs["content_kind"] != "text" || attrs["result"] != "succeeded" {
		t.Fatalf("unexpected receive attrs: %+v", attrs)
	}
	if attrs := spanAttrMap(applySpan); attrs["content_kind"] != "text" || attrs["result"] != "succeeded" {
		t.Fatalf("unexpected apply attrs: %+v", attrs)
	}

	// 2. Built-in Action trace + idempotency on alpha.
	var hist struct {
		Items []struct {
			ID string `json:"id"`
		} `json:"items"`
	}
	a.eventually(t, func() bool {
		_ = json.Unmarshal(a.get(t, "/v1/history?limit=10"), &hist)
		return len(hist.Items) > 0
	})
	histID := hist.Items[0].ID

	a.mu.Lock()
	a.traceSince = time.Now()
	a.mu.Unlock()
	actBody := `{"request_id":"trace-action-req-1"}`
	for i := 0; i < 2; i++ {
		code, raw := a.request(t, "POST", "/v1/history/"+histID+"/actions/builtin.text/upper", actBody)
		if code != 200 {
			t.Fatalf("action call %d failed: %d %s", i, code, raw)
		}
	}
	var actSpans []*tracepb.Span
	a.eventually(t, func() bool {
		actSpans = nil
		for _, s := range traceSpans(t, a) {
			if s.Name == "unic.action" {
				actSpans = append(actSpans, s)
			}
		}
		return len(actSpans) >= 1
	})
	time.Sleep(500 * time.Millisecond)
	allActSpans := traceSpans(t, a)
	actSpans = nil
	for _, s := range allActSpans {
		if s.Name == "unic.action" {
			actSpans = append(actSpans, s)
		}
	}
	var actCommits []*tracepb.Span
	if len(actSpans) > 0 {
		for _, s := range allActSpans {
			if s.Name == "unic.commit" && bytes.Equal(s.TraceId, actSpans[0].TraceId) {
				actCommits = append(actCommits, s)
			}
		}
	}
	if len(actSpans) != 1 || len(actCommits) != 1 {
		t.Fatalf("expected 1 unic.action and 1 unic.commit after idempotent retry, got actions=%d commits=%d", len(actSpans), len(actCommits))
	}
	if attrs := spanAttrMap(actSpans[0]); attrs["action_category"] != "builtin" || attrs["result"] != "succeeded" || attrs["content_kind"] != "text" {
		t.Fatalf("unexpected action attrs: %+v", attrs)
	}
	if !bytes.Equal(actCommits[0].ParentSpanId, actSpans[0].SpanId) {
		t.Fatalf("action commit not parented to unic.action")
	}

	// 3. Explicit submission cross-device propagation.
	a.mu.Lock()
	a.traceSince = time.Now()
	a.mu.Unlock()
	b.mu.Lock()
	b.traceSince = time.Now()
	b.mu.Unlock()
	traceSubmit(t, a, "trace-dist-explicit-1")

	var subSpan, subCommitSpan, subSendSpan *tracepb.Span
	a.eventually(t, func() bool {
		subSpan, subCommitSpan, subSendSpan = nil, nil, nil
		for _, s := range traceSpans(t, a) {
			switch s.Name {
			case "unic.submission":
				subSpan = s
			case "unic.commit":
				subCommitSpan = s
			case "unic.sync.send":
				subSendSpan = s
			}
		}
		return subSpan != nil && subCommitSpan != nil && subSendSpan != nil
	})
	if !bytes.Equal(subCommitSpan.ParentSpanId, subSpan.SpanId) || !bytes.Equal(subSendSpan.ParentSpanId, subCommitSpan.SpanId) {
		t.Fatalf("explicit submission sender chain mismatch")
	}
	var subRecvSpan, subApplySpan *tracepb.Span
	b.eventually(t, func() bool {
		subRecvSpan, subApplySpan = nil, nil
		for _, s := range traceSpans(t, b) {
			if !bytes.Equal(s.TraceId, subSpan.TraceId) {
				continue
			}
			switch s.Name {
			case "unic.sync.receive":
				subRecvSpan = s
			case "unic.apply":
				subApplySpan = s
			}
		}
		return subRecvSpan != nil && subApplySpan != nil
	})
	if !bytes.Equal(subRecvSpan.ParentSpanId, subSendSpan.SpanId) || !bytes.Equal(subApplySpan.ParentSpanId, subRecvSpan.SpanId) {
		t.Fatalf("explicit submission cross-device receiver chain mismatch")
	}

	// 4. Explicit history apply on alpha produces a root unic.apply span.
	a.mu.Lock()
	a.traceSince = time.Now()
	a.mu.Unlock()
	if code, raw := a.request(t, "POST", "/v1/history/"+histID+"/use", ""); code != 200 {
		t.Fatalf("use history failed: %d %s", code, raw)
	}
	var histApply *tracepb.Span
	a.eventually(t, func() bool {
		for _, s := range traceSpans(t, a) {
			if s.Name == "unic.apply" && len(s.ParentSpanId) == 0 {
				histApply = s
				return true
			}
		}
		return false
	})
	if attrs := spanAttrMap(histApply); attrs["result"] != "succeeded" || attrs["content_kind"] != "text" {
		t.Fatalf("unexpected history apply attrs: %+v", attrs)
	}
}

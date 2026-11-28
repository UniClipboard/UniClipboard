// Command mobile-diagnostics verifies local diagnostics, temporary debug capture,
// ZIP diagnostic bundle export, and OTLP resource/power/MetricKit telemetry
// across real mobile Engine processes against an isolated TLS OTLP receiver.
package main

import (
	"archive/zip"
	"crypto/sha256"
	"crypto/tls"
	"encoding/hex"
	"encoding/json"
	"encoding/pem"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"
	"sync"
	"time"

	collectmetrics "go.opentelemetry.io/proto/otlp/collector/metrics/v1"
	commonpb "go.opentelemetry.io/proto/otlp/common/v1"
	metricspb "go.opentelemetry.io/proto/otlp/metrics/v1"
	"google.golang.org/protobuf/encoding/protojson"
	"google.golang.org/protobuf/proto"

	"github.com/uniclipboard/unic/bindings/mobile"
)

const privateMarker = "private mobile diagnostic fixture never exported"

func must[T any](value T, err error) T {
	check(err)
	return value
}

func check(err error) {
	if err != nil {
		panic(err)
	}
}

func require(ok bool, message string) {
	if !ok {
		panic(message)
	}
}

func configured(state string) bool {
	var value struct {
		Configured bool `json:"configured_enabled"`
	}
	check(json.Unmarshal([]byte(state), &value))
	return value.Configured
}

func verifyArchive(m *mobile.Engine, profile, cacheDir string) {
	levelJSON := must(m.SetLocalLogLevelOverride("debug", 600, false))
	var levelState struct {
		Local struct {
			Effective string `json:"effective_level"`
			Source    string `json:"source"`
			Expires   *int64 `json:"expires_at_ms"`
		} `json:"local"`
	}
	check(json.Unmarshal([]byte(levelJSON), &levelState))
	require(levelState.Local.Effective == "debug" && levelState.Local.Source == "override" && levelState.Local.Expires != nil,
		"local debug capture override did not activate")

	archiveJSON := must(m.ExportDiagnosticArchive(`{"app_version":"2.0.0","app_build":"1","bundle_id":"app.uniclipboard.shell","platform":"mobile","os_version":"test"}`))
	var archive struct {
		Directory    string   `json:"directory"`
		Path         string   `json:"path"`
		FileName     string   `json:"file_name"`
		MIME         string   `json:"mime"`
		SizeBytes    int64    `json:"size_bytes"`
		LogFileCount int      `json:"log_file_count"`
		Warnings     []string `json:"warnings"`
	}
	check(json.Unmarshal([]byte(archiveJSON), &archive))
	defer func() { _ = os.RemoveAll(archive.Directory) }()
	require(archive.MIME == "application/zip" && strings.HasSuffix(archive.FileName, ".zip"), "unexpected diagnostic archive mime/name")
	require(archive.SizeBytes > 0 && archive.LogFileCount >= 1 && len(archive.Warnings) == 0, "expected diagnostic archive with disk logs and no warnings")

	zr := must(zip.OpenReader(archive.Path))
	defer func() { _ = zr.Close() }()
	entries := map[string][]byte{}
	for _, f := range zr.File {
		rc := must(f.Open())
		body := must(io.ReadAll(rc))
		_ = rc.Close()
		text := string(body)
		require(!strings.Contains(text, privateMarker) && !strings.Contains(text, profile) && !strings.Contains(text, cacheDir),
			"private data reached diagnostic archive")
		entries[f.Name] = body
	}
	manifestBytes, ok := entries["manifest.json"]
	require(ok, "diagnostic archive missing manifest.json")
	var manifest struct {
		FormatVersion int `json:"format_version"`
		EngineLogs    struct {
			Status string `json:"status"`
		} `json:"engine_logs"`
		Files []struct {
			Name   string `json:"name"`
			Bytes  int64  `json:"bytes"`
			SHA256 string `json:"sha256"`
		} `json:"files"`
	}
	check(json.Unmarshal(manifestBytes, &manifest))
	require(manifest.FormatVersion == 1 && manifest.EngineLogs.Status == "included" && len(manifest.Files) >= 1,
		"diagnostic archive manifest incomplete")
	foundStarted := false
	for _, entry := range manifest.Files {
		body, exists := entries[entry.Name]
		require(exists, "manifest file missing from zip")
		sum := sha256.Sum256(body)
		require(int64(len(body)) == entry.Bytes && hex.EncodeToString(sum[:]) == entry.SHA256, "diagnostic file digest mismatch")
		if strings.Contains(string(body), "engine.started") {
			foundStarted = true
		}
		for _, line := range strings.Split(strings.TrimSpace(string(body)), "\n") {
			require(json.Valid([]byte(line)), "diagnostic log entry must contain complete JSON lines")
		}
	}
	require(foundStarted, "diagnostic archive missing engine.started record")

	resetJSON := must(m.SetLocalLogLevelOverride("", 0, true))
	check(json.Unmarshal([]byte(resetJSON), &levelState))
	require(levelState.Local.Source == "config" && levelState.Local.Expires == nil, "local debug capture override did not reset")
}

func process(profile, cacheDir string, writer bool) {
	config := must(json.Marshal(map[string]string{"data_dir": profile, "cache_dir": cacheDir, "device_name": privateMarker}))
	m := must(mobile.NewEngine(string(config)))
	defer func() { check(m.Close(nil)) }()
	check(m.Start(nil))
	require(configured(must(m.GetTelemetryState())) == writer, "unexpected telemetry preference after launch")
	if writer {
		require(!configured(must(m.SetTelemetryEnabled(false))), "disabling telemetry did not apply")
	}
	deadline := time.Now().Add(4 * time.Second)
	for {
		logs := m.ExportRecentDiagnosticLogs()
		if strings.Contains(logs, "engine.started") {
			require(!strings.Contains(logs, privateMarker) && !strings.Contains(logs, profile), "private data reached local diagnostic export")
			for _, line := range strings.Split(strings.TrimSpace(logs), "\n") {
				require(json.Valid([]byte(line)), "diagnostics must contain complete JSON lines")
			}
			break
		}
		require(time.Now().Before(deadline), "local log export was unavailable")
		time.Sleep(20 * time.Millisecond)
	}
	verifyArchive(m, profile, cacheDir)
	if !writer {
		require(configured(must(m.SetTelemetryEnabled(true))), "enabling telemetry did not apply")
	}
}

type scriptedProvider struct {
	mu      sync.Mutex
	epochs  []int64
	primed  map[int64]bool
	queued  []string
	disable bool
}

func newScriptedProvider() *scriptedProvider {
	return &scriptedProvider{primed: map[int64]bool{}}
}

func (p *scriptedProvider) enqueue(payload string) {
	p.mu.Lock()
	p.queued = append(p.queued, payload)
	p.mu.Unlock()
}

func (p *scriptedProvider) lastEpoch() int64 {
	p.mu.Lock()
	defer p.mu.Unlock()
	if len(p.epochs) == 0 {
		return 0
	}
	return p.epochs[len(p.epochs)-1]
}

func (p *scriptedProvider) CollectResourceSnapshot(epoch int64) string {
	p.mu.Lock()
	defer p.mu.Unlock()
	p.epochs = append(p.epochs, epoch)
	if epoch <= 0 {
		p.queued = nil
		p.disable = true
		return ""
	}
	p.disable = false
	if !p.primed[epoch] {
		p.primed[epoch] = true
		p.queued = nil
		return ""
	}
	if len(p.queued) == 0 {
		return ""
	}
	next := p.queued[0]
	p.queued = p.queued[1:]
	return next
}

func runMobileMainWorker(profile, cacheDir string) {
	cfg := must(json.Marshal(map[string]any{
		"data_dir":       profile,
		"cache_dir":      cacheDir,
		"device_name":    privateMarker,
		"extension_mode": false,
	}))
	m := must(mobile.NewEngine(string(cfg)))
	check(m.Start(nil))

	prov := newScriptedProvider()
	m.SetHostResourceProvider(prov)
	require(prov.lastEpoch() > 0, "provider was not primed with positive epoch")
	prov.enqueue(`{"resource":{"app_state":"foreground","power_source":"battery","window_seconds":60.0,"cpu_seconds":1.5,"cpu_probe_status":"ok","memory_footprint_bytes":52428800,"memory_probe_status":"ok","wakeups":42,"wakeups_probe_status":"ok"},"power":{"app_state":"background_screen_off","power_source":"battery","tx_bytes":12000,"rx_bytes":34000,"network_probe_status":"ok","thermal_level":"fair","thermal_seconds":60,"thermal_probe_status":"ok","bg_mode":"fgs_idle","screen_state":"off","multicast_lock":true,"bg_mode_seconds":60,"battery_drain_rate":3.6,"low_power_mode":false,"battery_probe_status":"ok"}}`)

	// Flush 1: valid main-app resource + full power window.
	check(m.FlushResourceSample())

	// Missing/unsupported probes must increment probe_status without zero-filling.
	check(m.SubmitPowerWindow(`{"resource":{"app_state":"foreground","power_source":"battery","window_seconds":12.0,"cpu_seconds":0.2,"cpu_probe_status":"ok","memory_probe_status":"unsupported","wakeups_probe_status":"unsupported"},"power":{"app_state":"foreground","power_source":"unknown","network_probe_status":"unsupported","thermal_probe_status":"error","battery_probe_status":"unsupported"}}`))

	// Short (<10s) and sleep_jumped windows must record window/cpu time but NEVER cpu.utilization.
	check(m.SubmitPowerWindow(`{"resource":{"app_state":"foreground","power_source":"plugged","window_seconds":4.0,"cpu_seconds":0.4,"cpu_probe_status":"ok","memory_footprint_bytes":50000000,"memory_probe_status":"ok"}}`))
	check(m.SubmitPowerWindow(`{"resource":{"app_state":"foreground","power_source":"plugged","window_seconds":15.0,"cpu_seconds":0.5,"cpu_probe_status":"ok","sleep_jumped":true,"memory_footprint_bytes":50000000,"memory_probe_status":"ok"}}`))

	// Sanitized 24h iOS MetricKit summary (main app only).
	check(m.SubmitMetricKitPayload(`{"cpu_seconds":14.2,"wifi_tx_bytes":50000,"wifi_rx_bytes":120000,"cellular_tx_bytes":8000,"cellular_rx_bytes":16000,"peak_memory_bytes":134217728,"foreground_seconds":600,"background_seconds":1800}`))

	check(m.Close(nil))
}

func runMobileBarrierWorker(profile, cacheDir string) {
	cfg := must(json.Marshal(map[string]any{
		"data_dir":       profile,
		"cache_dir":      cacheDir,
		"device_name":    privateMarker,
		"extension_mode": false,
	}))
	m := must(mobile.NewEngine(string(cfg)))
	check(m.Start(nil))

	prov := newScriptedProvider()
	m.SetHostResourceProvider(prov)
	epochBefore := prov.lastEpoch()
	require(epochBefore > 0, "expected positive epoch before disable")

	// Disable barrier: must notify provider with epoch=0 and drop/suppress samples.
	require(!configured(must(m.SetTelemetryEnabled(false))), "disable failed")
	deadline := time.Now().Add(3 * time.Second)
	for prov.lastEpoch() != 0 {
		require(time.Now().Before(deadline), "disable barrier did not reset provider epoch to 0")
		time.Sleep(15 * time.Millisecond)
	}
	// Disabled-period CPU spike must NEVER be flushed or replayed.
	prov.enqueue(`{"resource":{"app_state":"background","power_source":"battery","window_seconds":300.0,"cpu_seconds":99.0,"cpu_probe_status":"ok","memory_footprint_bytes":11111111,"memory_probe_status":"ok"}}`)
	check(m.FlushResourceSample())
	check(m.SubmitPowerWindow(`{"resource":{"app_state":"background","power_source":"battery","window_seconds":300.0,"cpu_seconds":99.0,"cpu_probe_status":"ok"}}`))
	time.Sleep(500 * time.Millisecond)

	// Re-enable: must bump generation epoch cleanly and accept only fresh post-enable baseline.
	require(configured(must(m.SetTelemetryEnabled(true))), "reenable failed")
	deadline = time.Now().Add(3 * time.Second)
	for prov.lastEpoch() <= epochBefore {
		require(time.Now().Before(deadline), fmt.Sprintf("expected fresh epoch > %d after reenable, got %d", epochBefore, prov.lastEpoch()))
		time.Sleep(15 * time.Millisecond)
	}
	prov.enqueue(`{"resource":{"app_state":"foreground","power_source":"battery","window_seconds":25.0,"cpu_seconds":1.25,"cpu_probe_status":"ok","memory_footprint_bytes":41943040,"memory_probe_status":"ok"}}`)
	check(m.FlushResourceSample())
	check(m.Close(nil))
}

func runMobileKeyboardAndFallbackWorker(extProfile, extCache, fbProfile, fbCache string) {
	// 1. iOS Keyboard Extension mode via global RegisterHostResourceProvider.
	prov := newScriptedProvider()
	mobile.RegisterHostResourceProvider(prov)
	defer mobile.RegisterHostResourceProvider(nil)

	extCfg := must(json.Marshal(map[string]any{
		"data_dir":       extProfile,
		"cache_dir":      extCache,
		"device_name":    privateMarker,
		"extension_mode": true,
	}))
	ext := must(mobile.NewEngine(string(extCfg)))
	check(ext.Start(nil))
	require(ext.SubmitMetricKitPayload(`{"cpu_seconds":1.0}`) != nil, "extension mode must reject MetricKit")
	deadline := time.Now().Add(3 * time.Second)
	for prov.lastEpoch() <= 0 {
		require(time.Now().Before(deadline), "keyboard provider was not primed")
		time.Sleep(15 * time.Millisecond)
	}
	prov.enqueue(`{"resource":{"app_state":"foreground","power_source":"battery","window_seconds":6.0,"cpu_seconds":0.6,"cpu_probe_status":"ok","memory_footprint_bytes":16777216,"memory_probe_status":"ok"}}`)
	check(ext.FlushResourceSample())
	check(ext.Close(nil))

	// 2. Pure-Go procmetrics fallback (no HostResourceProvider registered).
	mobile.RegisterHostResourceProvider(nil)
	fbCfg := must(json.Marshal(map[string]any{
		"data_dir":       fbProfile,
		"cache_dir":      fbCache,
		"device_name":    privateMarker,
		"extension_mode": false,
	}))
	fb := must(mobile.NewEngine(string(fbCfg)))
	check(fb.Start(nil))
	check(fb.FlushResourceSample())
	require(!configured(must(fb.SetTelemetryEnabled(false))), "fallback disable failed")
	require(configured(must(fb.SetTelemetryEnabled(true))), "fallback reenable failed")
	check(fb.FlushResourceSample())
	check(fb.Close(nil))
}

type otlpCapture struct {
	mu      sync.Mutex
	reqs    []capturedReq
	metrics []*collectmetrics.ExportMetricsServiceRequest
}

type capturedReq struct {
	Path      string
	Timestamp time.Time
	RawBody   []byte
}

func (c *otlpCapture) handler(w http.ResponseWriter, r *http.Request) {
	body, _ := io.ReadAll(r.Body)
	req := capturedReq{Path: r.URL.Path, Timestamp: time.Now(), RawBody: body}
	c.mu.Lock()
	c.reqs = append(c.reqs, req)
	if r.URL.Path == "/i/v1/metrics" || r.URL.Path == "/v1/metrics" {
		var p collectmetrics.ExportMetricsServiceRequest
		if proto.Unmarshal(body, &p) == nil {
			c.metrics = append(c.metrics, &p)
		}
	}
	c.mu.Unlock()
	w.WriteHeader(200)
}

func (c *otlpCapture) snapshot() ([]capturedReq, []*collectmetrics.ExportMetricsServiceRequest) {
	c.mu.Lock()
	defer c.mu.Unlock()
	reqs := append([]capturedReq(nil), c.reqs...)
	mets := append([]*collectmetrics.ExportMetricsServiceRequest(nil), c.metrics...)
	return reqs, mets
}

func attrMap(attrs []*commonpb.KeyValue) map[string]string {
	out := map[string]string{}
	for _, a := range attrs {
		switch v := a.Value.Value.(type) {
		case *commonpb.AnyValue_StringValue:
			out[a.Key] = v.StringValue
		case *commonpb.AnyValue_BoolValue:
			if v.BoolValue {
				out[a.Key] = "true"
			} else {
				out[a.Key] = "false"
			}
		}
	}
	return out
}

func findBindingsDir() string {
	wd := must(os.Getwd())
	if _, err := os.Stat(filepath.Join(wd, "go.mod")); err == nil && filepath.Base(wd) == "bindings" {
		return wd
	}
	if _, err := os.Stat(filepath.Join(wd, "bindings", "go.mod")); err == nil {
		return filepath.Join(wd, "bindings")
	}
	return wd
}

func buildWorkerWithEndpoint(bindingsDir, outPath, endpoint string) {
	ldflags := fmt.Sprintf(
		"-X github.com/uniclipboard/unic/internal/observability.PostHogHost=%s -X github.com/uniclipboard/unic/internal/observability.PostHogToken=%s",
		endpoint,
		"phc_mobile_diag_e2e",
	)
	cmd := exec.Command("go", "build", "-ldflags", ldflags, "-o", outPath, "./e2e/mobile-diagnostics")
	cmd.Dir = bindingsDir
	cmd.Env = append(os.Environ(), "GOWORK=off")
	out, err := cmd.CombinedOutput()
	if err != nil {
		panic(fmt.Sprintf("build worker: %v\n%s", err, out))
	}
}

func runOTLPPhases(base, caPath, workerBin string) map[string]any {
	runWorker := func(mode string, extraArgs ...string) {
		args := append([]string{mode}, extraArgs...)
		cmd := exec.Command(workerBin, args...)
		cmd.Env = append(os.Environ(), "SSL_CERT_FILE="+caPath)
		cmd.Stdout, cmd.Stderr = os.Stdout, os.Stderr
		check(cmd.Run())
	}

	cap := &otlpCapture{}
	srv := httptest.NewUnstartedServer(http.HandlerFunc(cap.handler))
	srv.TLS = &tls.Config{MinVersion: tls.VersionTLS12}
	srv.StartTLS()
	defer srv.Close()
	caPEM := pem.EncodeToMemory(&pem.Block{Type: "CERTIFICATE", Bytes: srv.Certificate().Raw})
	check(os.WriteFile(caPath, caPEM, 0600))

	bindingsDir := findBindingsDir()
	buildWorkerWithEndpoint(bindingsDir, workerBin, srv.URL)

	// 1. Main app + power + missing/unsupported + sleep jump + MetricKit 24h summary.
	runWorker("otlp-main", filepath.Join(base, "m1-data"), filepath.Join(base, "m1-cache"))
	reqs1, mets1 := cap.snapshot()
	verifyMainOTLP(mets1, base)
	verifyPrivacy(reqs1, base)

	// 2. Disable barrier + re-enable baseline reset (no replay of disabled 99s CPU).
	beforeBarrierIdx := len(mets1)
	runWorker("otlp-barrier", filepath.Join(base, "m2-data"), filepath.Join(base, "m2-cache"))
	reqs2, mets2 := cap.snapshot()
	verifyBarrierOTLP(mets2[beforeBarrierIdx:])
	verifyPrivacy(reqs2, base)

	// 3. Keyboard extension forced role (ios_keyboard) + MetricKit reject + Go fallback.
	beforeExtIdx := len(mets2)
	runWorker("otlp-ext-fb",
		filepath.Join(base, "m3-ext-data"), filepath.Join(base, "m3-ext-cache"),
		filepath.Join(base, "m4-fb-data"), filepath.Join(base, "m4-fb-cache"),
	)
	reqs3, mets3 := cap.snapshot()
	verifyKeyboardAndFallbackOTLP(mets3[beforeExtIdx:])
	verifyPrivacy(reqs3, base)

	summary := buildMetricsSummary(mets3)
	writeArtifacts(reqs3, mets3, summary)
	return summary
}

func verifyPrivacy(reqs []capturedReq, base string) {
	allowedRes := "host.arch,os.type,service.name,service.version"
	for _, r := range reqs {
		bodyStr := string(r.RawBody)
		require(!strings.Contains(bodyStr, privateMarker) && !strings.Contains(bodyStr, base),
			"private string leaked in mobile OTLP payload")
		if r.Path == "/i/v1/metrics" || r.Path == "/v1/metrics" {
			var p collectmetrics.ExportMetricsServiceRequest
			check(proto.Unmarshal(r.RawBody, &p))
			for _, rm := range p.ResourceMetrics {
				var keys []string
				for _, kv := range rm.Resource.Attributes {
					keys = append(keys, kv.Key)
				}
				sort.Strings(keys)
				require(strings.Join(keys, ",") == allowedRes, fmt.Sprintf("unexpected mobile Resource keys: %v", keys))
			}
		}
	}
}

func iterMetrics(reqs []*collectmetrics.ExportMetricsServiceRequest, fn func(m *metricspb.Metric)) {
	for _, r := range reqs {
		for _, rm := range r.ResourceMetrics {
			for _, sm := range rm.ScopeMetrics {
				for _, m := range sm.Metrics {
					fn(m)
				}
			}
		}
	}
}

func verifyMainOTLP(mets []*collectmetrics.ExportMetricsServiceRequest, _ string) {
	var (
		cpuTotal     float64
		winTotal     float64
		utilCount    uint64
		memCount     uint64
		heapCount    uint64
		wakeups      int64
		netTotal     int64
		hasBattery   bool
		hasBgMode    bool
		hasThermal   bool
		probeSeen    = map[string]int64{}
		mkMetrics    = map[string]bool{}
		mkTransports = map[string]bool{}
	)
	iterMetrics(mets, func(m *metricspb.Metric) {
		switch m.Name {
		case "unic.resource.cpu.time":
			for _, dp := range m.GetSum().GetDataPoints() {
				require(attrMap(dp.Attributes)["process_role"] == "mobile_app", "expected mobile_app role")
				cpuTotal += dp.GetAsDouble()
			}
		case "unic.resource.window.time":
			for _, dp := range m.GetSum().GetDataPoints() {
				winTotal += dp.GetAsDouble()
			}
		case "unic.resource.cpu.utilization":
			for _, dp := range m.GetHistogram().GetDataPoints() {
				utilCount += dp.Count
			}
		case "unic.resource.memory.footprint":
			for _, dp := range m.GetHistogram().GetDataPoints() {
				memCount += dp.Count
			}
		case "unic.resource.memory.go_heap":
			for _, dp := range m.GetHistogram().GetDataPoints() {
				heapCount += dp.Count
			}
		case "unic.power.wakeups":
			for _, dp := range m.GetSum().GetDataPoints() {
				wakeups += dp.GetAsInt()
			}
		case "unic.resource.probe_status":
			for _, dp := range m.GetSum().GetDataPoints() {
				a := attrMap(dp.Attributes)
				probeSeen[a["probe"]+":"+a["status"]] += dp.GetAsInt()
			}
		case "unic.power.network.bytes":
			for _, dp := range m.GetSum().GetDataPoints() {
				a := attrMap(dp.Attributes)
				_, hasTransport := a["transport"]
				require(!hasTransport, "unic.power.network.bytes must NOT emit guessed transport attribute")
				netTotal += dp.GetAsInt()
			}
		case "unic.power.thermal.time":
			for _, dp := range m.GetSum().GetDataPoints() {
				if attrMap(dp.Attributes)["level"] == "fair" {
					hasThermal = true
				}
			}
		case "unic.power.bg_mode.time":
			for _, dp := range m.GetSum().GetDataPoints() {
				a := attrMap(dp.Attributes)
				if a["mode"] == "fgs_idle" && a["screen_state"] == "off" && a["multicast_lock"] == "true" {
					hasBgMode = true
				}
			}
		case "unic.power.system_battery.drain_rate":
			for _, dp := range m.GetHistogram().GetDataPoints() {
				a := attrMap(dp.Attributes)
				if a["app_state"] == "background_screen_off" && a["low_power_mode"] == "false" && dp.Count > 0 {
					hasBattery = true
				}
			}
		default:
			if strings.HasPrefix(m.Name, "unic.power.ios_metrickit.") {
				mkMetrics[m.Name] = true
				if m.Name == "unic.power.ios_metrickit.network.bytes" {
					for _, dp := range m.GetSum().GetDataPoints() {
						mkTransports[attrMap(dp.Attributes)["transport"]] = true
					}
				}
			}
		}
	})
	require(cpuTotal > 2.09 && cpuTotal < 2.11 && winTotal == 76.0, fmt.Sprintf("unexpected main cpu/window totals (expected 2.1/76.0 with sleep_jumped excluded): cpu=%v win=%v", cpuTotal, winTotal))
	require(utilCount == 2, fmt.Sprintf("short (<10s) and sleep_jumped windows must be omitted from cpu.utilization, got count=%d", utilCount))
	require(memCount == 3 && heapCount == 4, fmt.Sprintf("unsupported memory must not emit footprint histogram (got mem=%d heap=%d)", memCount, heapCount))
	require(wakeups == 42, fmt.Sprintf("expected wakeups=42 without zero-fill from unsupported, got %d", wakeups))
	require(probeSeen["wakeups:unsupported"] == 1 && probeSeen["memory:unsupported"] == 1 &&
		probeSeen["network:unsupported"] == 1 && probeSeen["thermal:error"] == 1 && probeSeen["battery:unsupported"] == 1,
		fmt.Sprintf("missing expected probe_status counters: %+v", probeSeen))
	require(netTotal == 46000 && hasThermal && hasBgMode && hasBattery,
		fmt.Sprintf("incomplete power proxy/system metrics: net=%d thermal=%t bg=%t bat=%t", netTotal, hasThermal, hasBgMode, hasBattery))
	require(mkMetrics["unic.power.ios_metrickit.cpu.time"] &&
		mkMetrics["unic.power.ios_metrickit.network.bytes"] &&
		mkMetrics["unic.power.ios_metrickit.memory.peak"] &&
		mkMetrics["unic.power.ios_metrickit.app_time"] &&
		mkTransports["wifi"] && mkTransports["cellular"],
		fmt.Sprintf("missing MetricKit summary metrics/transports: metrics=%+v transports=%+v", mkMetrics, mkTransports))
}

func verifyBarrierOTLP(mets []*collectmetrics.ExportMetricsServiceRequest) {
	var cpuTotal, winTotal float64
	iterMetrics(mets, func(m *metricspb.Metric) {
		switch m.Name {
		case "unic.resource.cpu.time":
			for _, dp := range m.GetSum().GetDataPoints() {
				cpuTotal += dp.GetAsDouble()
			}
		case "unic.resource.window.time":
			for _, dp := range m.GetSum().GetDataPoints() {
				winTotal += dp.GetAsDouble()
			}
		}
	})
	require(cpuTotal == 1.25 && winTotal == 25.0,
		fmt.Sprintf("disable barrier failed or replayed disabled 99s CPU: cpu=%v win=%v", cpuTotal, winTotal))
}

func verifyKeyboardAndFallbackOTLP(mets []*collectmetrics.ExportMetricsServiceRequest) {
	var (
		kbCPU    float64
		kbUtil   uint64
		fbGoHeap uint64
		kbSeenMK bool
	)
	iterMetrics(mets, func(m *metricspb.Metric) {
		if strings.HasPrefix(m.Name, "unic.power.ios_metrickit.") {
			kbSeenMK = true
		}
		switch m.Name {
		case "unic.resource.cpu.time":
			for _, dp := range m.GetSum().GetDataPoints() {
				if attrMap(dp.Attributes)["process_role"] == "ios_keyboard" {
					kbCPU += dp.GetAsDouble()
				}
			}
		case "unic.resource.cpu.utilization":
			for _, dp := range m.GetHistogram().GetDataPoints() {
				if attrMap(dp.Attributes)["process_role"] == "ios_keyboard" {
					kbUtil += dp.Count
				}
			}
		case "unic.resource.memory.go_heap":
			for _, dp := range m.GetHistogram().GetDataPoints() {
				if attrMap(dp.Attributes)["process_role"] == "mobile_app" {
					fbGoHeap += dp.Count
				}
			}
		}
	})
	require(!kbSeenMK, "keyboard extension / fallback run must never emit MetricKit metrics")
	require(kbCPU == 0.6 && kbUtil == 1, fmt.Sprintf("expected ios_keyboard cpu=0.6 util=1, got cpu=%v util=%d", kbCPU, kbUtil))
	require(fbGoHeap >= 1, "expected pure-Go fallback mobile_app go_heap sample after reenable")
}

func buildMetricsSummary(mets []*collectmetrics.ExportMetricsServiceRequest) map[string]any {
	names := map[string]int{}
	roles := map[string]int{}
	iterMetrics(mets, func(m *metricspb.Metric) {
		names[m.Name]++
		for _, dp := range m.GetSum().GetDataPoints() {
			if r := attrMap(dp.Attributes)["process_role"]; r != "" {
				roles[r]++
			}
		}
		for _, dp := range m.GetHistogram().GetDataPoints() {
			if r := attrMap(dp.Attributes)["process_role"]; r != "" {
				roles[r]++
			}
		}
	})
	return map[string]any{
		"default_enabled":               true,
		"disable_persisted":             true,
		"reenabled":                     true,
		"local_log_export":              "zip_bundle",
		"debug_capture_override":        true,
		"otlp_resource_power_verified":  true,
		"disable_barrier_no_replay":     true,
		"missing_probe_no_zerofill":     true,
		"keyboard_extension_role_bound": true,
		"metrickit_summary_verified":    true,
		"metric_series_counts":          names,
		"process_role_datapoints":       roles,
	}
}

func writeArtifacts(reqs []capturedReq, mets []*collectmetrics.ExportMetricsServiceRequest, summary map[string]any) {
	artDir := os.Getenv("UNIC_MOBILE_DIAG_ARTIFACTS")
	if artDir == "" {
		return
	}
	check(os.MkdirAll(artDir, 0700))

	decoded := make([]json.RawMessage, 0, len(mets))
	mo := protojson.MarshalOptions{EmitUnpopulated: false}
	for _, m := range mets {
		b := must(mo.Marshal(m))
		decoded = append(decoded, json.RawMessage(b))
	}
	decodedJSON := must(json.MarshalIndent(decoded, "", "  "))
	check(os.WriteFile(filepath.Join(artDir, "otlp-decoded.json"), decodedJSON, 0600))

	summaryJSON := must(json.MarshalIndent(summary, "", "  "))
	check(os.WriteFile(filepath.Join(artDir, "summary.json"), summaryJSON, 0600))

	assertions := []map[string]any{
		{"scenario": "local-diagnostics-zip-and-preference-persistence", "passed": true},
		{"scenario": "mobile-main-power-missing-sleepjump-and-metrickit", "passed": true, "otlp_requests": len(reqs)},
		{"scenario": "mobile-disable-barrier-and-reenable-baseline-reset", "passed": true},
		{"scenario": "mobile-keyboard-extension-role-and-go-fallback", "passed": true},
	}
	check(os.WriteFile(filepath.Join(artDir, "assertions.json"), must(json.MarshalIndent(assertions, "", "  ")), 0600))

	replay := "#!/usr/bin/env sh\nset -eu\n" +
		"cd \"$(dirname \"$0\")/../../..\"\n" +
		"cd bindings\n" +
		"GOWORK=off UNIC_MOBILE_DIAG_ARTIFACTS=\"../.cache/0037/mobile-diagnostics\" go run ./e2e/mobile-diagnostics\n"
	check(os.WriteFile(filepath.Join(artDir, "replay.sh"), []byte(replay), 0700))

	var sums strings.Builder
	for _, name := range []string{"assertions.json", "otlp-decoded.json", "replay.sh", "summary.json"} {
		b := must(os.ReadFile(filepath.Join(artDir, name)))
		h := sha256.Sum256(b)
		fmt.Fprintf(&sums, "%s  %s\n", hex.EncodeToString(h[:]), name)
	}
	check(os.WriteFile(filepath.Join(artDir, "SHA256SUMS"), []byte(sums.String()), 0600))
}

func main() {
	if len(os.Args) >= 2 {
		switch os.Args[1] {
		case "write", "read":
			require(len(os.Args) == 4, "usage: write|read <profile> <cacheDir>")
			process(os.Args[2], os.Args[3], os.Args[1] == "write")
			return
		case "otlp-main":
			require(len(os.Args) == 4, "usage: otlp-main <profile> <cacheDir>")
			runMobileMainWorker(os.Args[2], os.Args[3])
			return
		case "otlp-barrier":
			require(len(os.Args) == 4, "usage: otlp-barrier <profile> <cacheDir>")
			runMobileBarrierWorker(os.Args[2], os.Args[3])
			return
		case "otlp-ext-fb":
			require(len(os.Args) == 6, "usage: otlp-ext-fb <extProfile> <extCache> <fbProfile> <fbCache>")
			runMobileKeyboardAndFallbackWorker(os.Args[2], os.Args[3], os.Args[4], os.Args[5])
			return
		}
	}

	dir := must(os.MkdirTemp("", "unic-mobile-diagnostics-"))
	defer os.RemoveAll(dir)
	resolved := must(filepath.EvalSymlinks(dir))
	profile := filepath.Join(resolved, "profile")
	cacheDir := filepath.Join(resolved, "cache")
	executable := must(os.Executable())
	for _, phase := range []string{"write", "read"} {
		cmd := exec.Command(executable, phase, profile, cacheDir)
		cmd.Stdout, cmd.Stderr = os.Stdout, os.Stderr
		check(cmd.Run())
	}

	summary := runOTLPPhases(resolved, filepath.Join(resolved, "ca.pem"), filepath.Join(resolved, "mobile-diagnostics-worker"))
	fmt.Println(string(must(json.Marshal(summary))))
}

package e2e

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"encoding/pem"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"sort"
	"strings"
	"sync"
	"syscall"
	"testing"
	"time"

	collectlogs "go.opentelemetry.io/proto/otlp/collector/logs/v1"
	collectmetrics "go.opentelemetry.io/proto/otlp/collector/metrics/v1"
	"google.golang.org/protobuf/encoding/protojson"
	"google.golang.org/protobuf/proto"
)

// These scenarios are defined before implementation. They use the same daemon
// host in a build-tagged nonvisual entry, never mygo.App or the system clipboard.
func TestObservability(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	h := newObservabilityRun(t)
	h.run(t, "default-off", func(t *testing.T) {
		h.start(t, "")
		h.cli(t, "logs", "level")
		h.cli(t, "diagnostics", "telemetry")
		h.requireNoRequests(t, 200*time.Millisecond)
	})
	h.run(t, "invalid-input", func(t *testing.T) {
		before := h.get(t, "/v1/diagnostics/log-level")
		for _, body := range []string{`{}`, `null`, `{"target":"local","target":"remote","level":"debug","duration_seconds":1}`, `{"target":"local","level":"DEBUG","duration_seconds":1}`, `{"target":"local","level":"debug","duration_seconds":1.5}`, `{"target":"local","level":"debug","duration_seconds":0}`, `{"target":"local","level":"debug","duration_seconds":3601}`, `{"target":"local","level":"debug","duration_seconds":1,"extra":true}`} {
			code, _ := h.request(t, "PUT", "/v1/diagnostics/log-level", body)
			if code != 400 {
				t.Fatalf("invalid input accepted: %d", code)
			}
		}
		if string(before) != string(h.get(t, "/v1/diagnostics/log-level")) {
			t.Fatal("invalid input changed state")
		}
	})
	h.run(t, "level-override", func(t *testing.T) {
		h.cli(t, "logs", "level", "debug", "--for", "1s")
		h.contains(t, "/v1/diagnostics/log-level", `"source":"override"`)
		h.cli(t, "config", "set", "logging.level", `"error"`)
		time.Sleep(1100 * time.Millisecond)
		h.contains(t, "/v1/diagnostics/log-level", `"effective_level":"error"`)
		h.cli(t, "logs", "level", "debug", "--for", "1s")
		h.cli(t, "logs", "level", "warn", "--for", "3s")
		time.Sleep(1100 * time.Millisecond)
		h.contains(t, "/v1/diagnostics/log-level", `"effective_level":"warn"`)
		h.cli(t, "logs", "level", "reset")
	})
	h.run(t, "remote-logs-metrics", func(t *testing.T) {
		h.stop(t)
		h.start(t, h.server.URL)
		h.cli(t, "logs", "level", "debug", "--target", "remote", "--for", "15m")
		h.cli(t, "config", "set", "telemetry.logs.enabled", "true")
		h.cli(t, "config", "set", "telemetry.enabled", "true")
		h.cli(t, "copy", "otlp-synthetic-content-secret")
		h.eventually(t, func() bool { return h.hasPath("/v1/logs") && h.hasPath("/v1/metrics") })
		h.assertOTLP(t)
	})
	h.run(t, "log-level-branches", func(t *testing.T) {
		h.cli(t, "logs", "level", "error", "--for", "1h")
		h.cli(t, "logs", "level", "debug", "--target", "remote", "--for", "1h")
		mark := time.Now()
		code, _ := h.request(t, "POST", "/test/logs", "")
		if code != 200 {
			t.Fatal("fixture failed")
		}
		h.eventually(t, func() bool { return h.hasLogSince("pipeline.completed", mark) })
		h.cli(t, "logs", "level", "reset")
	})
	h.run(t, "metrics-idempotency", func(t *testing.T) {
		before := h.metricCount("commit", "succeeded")
		code, _ := h.request(t, "POST", "/test/commit", "")
		if code != 200 {
			t.Fatal("commit fixture failed")
		}
		h.eventually(t, func() bool { return h.metricCount("commit", "succeeded") == before+1 })
		time.Sleep(600 * time.Millisecond)
		if h.metricCount("commit", "succeeded") != before+1 {
			t.Fatal("idempotent commit was counted twice")
		}
	})
	h.run(t, "invalid-diagnostic", func(t *testing.T) {
		before := h.signalDrops(t, "metrics")
		code, _ := h.request(t, "POST", "/test/invalid-record", "")
		if code != 200 {
			t.Fatal("fixture failed")
		}
		if h.signalDrops(t, "metrics") != before+1 {
			t.Fatal("invalid diagnostic was not counted")
		}
	})
	h.run(t, "queue-pressure", func(t *testing.T) {
		h.mu.Lock()
		h.status = 503
		h.mu.Unlock()
		before := h.signalDrops(t, "logs")
		code, _ := h.request(t, "POST", "/test/pressure", "")
		if code != 200 {
			t.Fatal("pressure fixture failed")
		}
		h.eventually(t, func() bool { return h.signalDrops(t, "logs") > before })
		h.cli(t, "copy", "pressure-synthetic-copy")
		h.mu.Lock()
		h.status = 200
		h.mu.Unlock()
	})
	h.run(t, "resume", func(t *testing.T) {
		h.cli(t, "logs", "level", "debug", "--for", "1s")
		_ = h.cmd.Process.Signal(syscall.SIGSTOP)
		time.Sleep(1100 * time.Millisecond)
		_ = h.cmd.Process.Signal(syscall.SIGCONT)
		h.contains(t, "/v1/diagnostics/log-level", `"source":"config"`)
	})
	h.run(t, "privacy", func(t *testing.T) {
		h.mu.Lock()
		defer h.mu.Unlock()
		for _, r := range h.requests {
			for _, secret := range []string{"otlp-synthetic-content-secret", "otlp-env-secret", "fixture-private-attribute", "fixture-private-message", "fixture-private-clipboard", "fixture-group-secret", h.home} {
				if strings.Contains(string(r.Body), secret) {
					t.Fatal("secret in telemetry")
				}
			}
		}
	})
	h.run(t, "disable-barrier", func(t *testing.T) {
		h.cli(t, "config", "set", "telemetry.enabled", "false")
		h.requireNoRequests(t, 1200*time.Millisecond)
	})
	h.run(t, "endpoint-restart", func(t *testing.T) {
		h.cli(t, "config", "set", "telemetry.endpoint", `"https://127.0.0.1:1"`)
		h.contains(t, "/v1/diagnostics/telemetry", `"telemetry.endpoint"`)
		h.cli(t, "config", "set", "telemetry.enabled", "true")
		h.cli(t, "copy", "endpoint-synthetic-copy")
		h.eventually(t, func() bool { return h.hasRecentPath("/v1/metrics") })
	})
	h.run(t, "export-failures", func(t *testing.T) {
		h.mu.Lock()
		h.status = 429
		h.mu.Unlock()
		h.eventually(t, func() bool { return strings.Contains(string(h.get(t, "/v1/diagnostics/telemetry")), `"degraded"`) })
		h.cli(t, "copy", "otlp-second-synthetic-secret")
		h.mu.Lock()
		h.status = 200
		h.mu.Unlock()
	})
	h.run(t, "lifecycle", func(t *testing.T) {
		h.sampleResources(t, "before-reconfigure")
		for i := 0; i < 100; i++ {
			h.cli(t, "config", "set", "telemetry.enabled", "false")
			h.cli(t, "config", "set", "telemetry.enabled", "true")
		}
		h.sampleResources(t, "after-reconfigure")
		h.stop(t)
		h.start(t, "")
		h.contains(t, "/v1/diagnostics/log-level", `"source":"config"`)
	})
	h.run(t, "local-retention", func(t *testing.T) {
		h.stop(t)
		logDir := filepath.Join(h.home, "logs")
		_ = os.MkdirAll(logDir, 0700)
		for i := 0; i < 6; i++ {
			name := fmt.Sprintf("segment-%d.jsonl", time.Now().Add(-8*24*time.Hour).UnixNano()+int64(i))
			if err := os.WriteFile(filepath.Join(logDir, name), make([]byte, 4<<20), 0600); err != nil {
				t.Fatal(err)
			}
		}
		h.start(t, "")
		h.eventually(t, func() bool {
			entries, _ := os.ReadDir(logDir)
			var total int64
			for _, e := range entries {
				info, err := e.Info()
				if err != nil {
					return false
				}
				total += info.Size()
			}
			return total <= 20<<20
		})
		h.stop(t)
		if err := os.Rename(logDir, logDir+"-saved"); err != nil {
			t.Fatal(err)
		}
		protected := filepath.Join(h.home, "protected")
		_ = os.MkdirAll(protected, 0700)
		marker := filepath.Join(protected, "marker")
		_ = os.WriteFile(marker, []byte("keep"), 0600)
		if err := os.Symlink(protected, logDir); err != nil {
			t.Fatal(err)
		}
		h.start(t, "")
		h.cli(t, "logs", "level", "debug", "--for", "15m")
		code, _ := h.request(t, "POST", "/test/logs", "")
		if code != 200 {
			t.Fatal("fixture failed")
		}
		h.eventually(t, func() bool {
			var st struct {
				Local struct {
					Dropped uint64 `json:"dropped_records"`
				} `json:"local"`
			}
			_ = json.Unmarshal(h.get(t, "/v1/diagnostics/log-level"), &st)
			return st.Local.Dropped > 0
		})
		b, err := os.ReadFile(marker)
		if err != nil || string(b) != "keep" {
			t.Fatal("log writer escaped private root")
		}
		entries, err := os.ReadDir(protected)
		if err != nil || len(entries) != 1 {
			t.Fatal("log writer followed profile-internal symlink")
		}
	})
}

type otlpRequest struct {
	Path       string
	At         time.Time
	Body       []byte
	Authorized bool
}
type observabilityRun struct {
	responseDelay    time.Duration
	responseLocation string
	untrusted        bool
	sourceHashes     map[string]string
	scenarioResults  map[string]string
	assertions       []map[string]any
	resourceSamples  []map[string]any

	artifactName          string
	production            bool
	executable            string
	posthog               bool
	manualAdapter         bool
	t                     *testing.T
	dir, home, sock, cert string
	server                *httptest.Server
	mu                    sync.Mutex
	requests              []otlpRequest
	status                int
	cmd                   *exec.Cmd
	done                  chan error
}

func newObservabilityRun(t *testing.T) *observabilityRun {
	d, err := os.MkdirTemp("/private/tmp", "uo")
	if err != nil {
		t.Fatal(err)
	}
	h := &observabilityRun{t: t, artifactName: "observability", sourceHashes: observabilitySourceHashes(t), scenarioResults: map[string]string{}, dir: d, home: filepath.Join(d, "home"), sock: filepath.Join(d, "s"), status: 200}
	if err := os.MkdirAll(h.home, 0700); err != nil {
		t.Fatal(err)
	}
	h.server = httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		b, _ := io.ReadAll(io.LimitReader(r.Body, 8<<20))
		authorized := r.Header.Get("Authorization") == "Bearer phc_unic_e2e_only"
		if r.URL.Path == "/batch/" {
			var batch map[string]any
			if json.Unmarshal(b, &batch) == nil {
				authorized = batch["api_key"] == "phc_unic_e2e_only"
				delete(batch, "api_key")
				b, _ = json.Marshal(batch)
			}
		}
		h.mu.Lock()
		h.requests = append(h.requests, otlpRequest{Path: r.URL.Path, At: time.Now(), Body: b, Authorized: authorized})
		status := h.status
		delay := h.responseDelay
		location := h.responseLocation
		h.mu.Unlock()
		if location != "" {
			w.Header().Set("Location", location)
		}
		if delay > 0 {
			select {
			case <-r.Context().Done():
				return
			case <-time.After(delay):
			}
		}
		w.Header().Set("Content-Type", "application/x-protobuf")
		w.WriteHeader(status)
	}))
	h.cert = filepath.Join(d, "ca.pem")
	_ = os.WriteFile(h.cert, pem.EncodeToMemory(&pem.Block{Type: "CERTIFICATE", Bytes: h.server.Certificate().Raw}), 0600)
	t.Cleanup(func() {
		if h.cmd != nil {
			h.stop(t)
		}
		h.server.Close()
		h.artifacts()
		_ = os.RemoveAll(d)
	})
	return h
}
func (h *observabilityRun) env() []string {
	cert := h.cert
	if h.untrusted {
		cert = "/dev/null"
	}
	fast := "1"
	if h.production {
		fast = ""
	}
	board := filepath.Join(h.home, "board")
	if h.manualAdapter {
		board = ""
	}
	return append(os.Environ(), "UNIC_HOME="+h.home, "UNIC_SOCK="+h.sock, "UNIC_MOCK_CLIPBOARD_DIR="+board, "SSL_CERT_FILE="+cert, "UNIC_E2E_DIAGNOSTICS=1", "UNIC_E2E_POSTHOG="+fmt.Sprint(h.posthog), "UNIC_E2E_FAST_DIAGNOSTICS="+fast, "GORACE=atexit_sleep_ms=0", "OTEL_RESOURCE_ATTRIBUTES=secret=otlp-env-secret", "OTEL_EXPORTER_OTLP_ENDPOINT=https://127.0.0.1:1")
}
func (h *observabilityRun) start(t *testing.T, endpoint string) {
	cfg := "[logging]\nlevel=\"info\"\n[telemetry]\nenabled=false\nendpoint=" + fmt.Sprintf("%q", endpoint) + "\n[telemetry.logs]\nenabled=false\n[telemetry.analytics]\nenabled=false\n"
	h.startConfig(t, cfg)
}
func (h *observabilityRun) startConfig(t *testing.T, cfg string) {
	if err := os.WriteFile(filepath.Join(h.home, "config.toml"), []byte(cfg), 0600); err != nil {
		t.Fatal(err)
	}
	exe := h.executable
	if exe == "" {
		exe = filepath.Join(binDir, "unicd")
	}
	cmd := exec.Command(exe)
	cmd.Env = h.env()
	f, err := os.Create(filepath.Join(h.dir, "daemon.log"))
	if err != nil {
		t.Fatal(err)
	}
	cmd.Stdout = f
	cmd.Stderr = f
	if err := cmd.Start(); err != nil {
		t.Fatal(err)
	}
	h.cmd = cmd
	h.done = make(chan error, 1)
	go func() { h.done <- cmd.Wait(); _ = f.Close() }()
	h.eventually(t, func() bool { _, err := os.Stat(h.sock); return err == nil })
}
func (h *observabilityRun) stop(t *testing.T) {
	if h.cmd == nil {
		return
	}
	_ = h.cmd.Process.Signal(syscall.SIGTERM)
	select {
	case err := <-h.done:
		if err != nil {
			t.Errorf("daemon exit: %v", err)
		}
	case <-time.After(5 * time.Second):
		_ = h.cmd.Process.Kill()
		if err := <-h.done; err != nil {
			t.Logf("forced daemon shutdown: %v", err)
		}
		t.Error("daemon exit timeout")
	}
	h.cmd = nil
}
func (h *observabilityRun) cli(t *testing.T, args ...string) string {
	t.Helper()
	cmd := exec.Command(filepath.Join(binDir, "unic"), args...)
	cmd.Env = h.env()
	b, err := cmd.CombinedOutput()
	if err != nil {
		t.Fatalf("unic %s failed: %v", args[0], err)
	}
	return string(b)
}
func (h *observabilityRun) request(t *testing.T, method, path, body string) (int, []byte) {
	t.Helper()
	c := &http.Client{Timeout: 5 * time.Second, Transport: &http.Transport{DialContext: func(ctx context.Context, _, _ string) (net.Conn, error) {
		return (&net.Dialer{}).DialContext(ctx, "unix", h.sock)
	}}}
	defer c.CloseIdleConnections()
	r, _ := http.NewRequest(method, "http://local"+path, strings.NewReader(body))
	r.Header.Set("Content-Type", "application/json")
	resp, err := c.Do(r)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = resp.Body.Close() }()
	b, _ := io.ReadAll(resp.Body)
	return resp.StatusCode, b
}
func (h *observabilityRun) get(t *testing.T, path string) []byte {
	t.Helper()
	code, b := h.request(t, "GET", path, "")
	if code != 200 {
		t.Fatalf("GET %s: %d %s", path, code, b)
	}
	return b
}
func (h *observabilityRun) contains(t *testing.T, path, want string) {
	t.Helper()
	b := h.get(t, path)
	h.assertions = append(h.assertions, map[string]any{"name": "state contains", "expected": want, "actual": json.RawMessage(b), "passed": strings.Contains(string(b), want)})
	if !strings.Contains(string(b), want) {
		t.Fatalf("%s missing %s: %s", path, want, b)
	}
}
func (h *observabilityRun) eventually(t *testing.T, fn func() bool) {
	t.Helper()
	end := time.Now().Add(85 * time.Second)
	for time.Now().Before(end) {
		if fn() {
			return
		}
		select {
		case err := <-h.done:
			t.Fatalf("daemon stopped: %v", err)
		default:
		}
		time.Sleep(100 * time.Millisecond)
	}
	t.Fatal("condition timeout")
}
func (h *observabilityRun) hasPath(p string) bool {
	h.mu.Lock()
	defer h.mu.Unlock()
	for _, r := range h.requests {
		if r.Path == p {
			return true
		}
	}
	return false
}
func (h *observabilityRun) hasRecentPath(p string) bool {
	h.mu.Lock()
	defer h.mu.Unlock()
	for _, r := range h.requests {
		if r.Path == p && time.Since(r.At) < 2*time.Second {
			return true
		}
	}
	return false
}
func (h *observabilityRun) requireNoRequests(t *testing.T, d time.Duration) {
	t.Helper()
	h.mu.Lock()
	n := len(h.requests)
	h.mu.Unlock()
	time.Sleep(d)
	h.mu.Lock()
	defer h.mu.Unlock()
	if len(h.requests) != n {
		t.Fatal("request after disabled barrier")
	}
}
func (h *observabilityRun) artifacts() {
	dest := os.Getenv("UNIC_E2E_ARTIFACTS")
	if dest == "" {
		return
	}
	d := filepath.Join(dest, h.artifactName)
	_ = os.MkdirAll(d, 0700)
	for _, n := range []string{"unic", "unicd"} {
		binaryPath := filepath.Join(binDir, n)
		if n == "unicd" && h.executable != "" {
			binaryPath = h.executable
		}
		b, err := os.ReadFile(binaryPath)
		if err == nil {
			sum := sha256.Sum256(b)
			_ = os.WriteFile(filepath.Join(d, n+".sha256"), []byte(hex.EncodeToString(sum[:])), 0600)
		}
	}
	h.mu.Lock()
	defer h.mu.Unlock()
	var decoded []map[string]any
	for _, r := range h.requests {
		if r.Path == "/batch/" {
			var data any
			_ = json.Unmarshal(r.Body, &data)
			decoded = append(decoded, map[string]any{"path": r.Path, "at": r.At, "data": data})
			continue
		}
		var msg proto.Message
		if r.Path == "/v1/logs" || r.Path == "/i/v1/logs" {
			msg = &collectlogs.ExportLogsServiceRequest{}
		} else {
			msg = &collectmetrics.ExportMetricsServiceRequest{}
		}
		if proto.Unmarshal(r.Body, msg) != nil {
			continue
		}
		j, _ := protojson.Marshal(msg)
		safe := true
		for _, needle := range []string{"otlp-env-secret", "fixture-private-attribute", "fixture-private-message", "fixture-group-secret", "fixture-private-clipboard", "pipeline-private-synthetic-content", h.home} {
			if strings.Contains(string(j), needle) {
				safe = false
				h.t.Error("unsafe telemetry payload omitted from artifact")
				break
			}
		}
		if !safe {
			continue
		}
		var obj any
		_ = json.Unmarshal(j, &obj)
		decoded = append(decoded, map[string]any{"path": r.Path, "at": r.At, "data": obj})
	}
	b, _ := json.MarshalIndent(decoded, "", "  ")
	_ = os.WriteFile(filepath.Join(d, "otlp-decoded.json"), b, 0600)
	b, _ = json.Marshal(map[string]any{"failed": h.t.Failed(), "requests": len(h.requests), "scenarios": h.scenarioResults, "replay": "TMPDIR=/private/tmp UNIC_E2E=1 go test -race ./e2e/cli -run TestObservability -count=1 -v"})
	_ = os.WriteFile(filepath.Join(d, "summary.json"), b, 0600)
	manifest := map[string]any{"source_files_sha256": h.sourceHashes, "source_unchanged": sameHashes(h.sourceHashes, observabilitySourceHashes(h.t)), "go": runtime.Version(), "os": runtime.GOOS, "arch": runtime.GOARCH, "test_daemon_build_tag": "unic_e2e", "metrics_interval_ms": 500, "log_interval_ms": 100, "race": true}
	root, _ := repoRoot()
	head, _ := exec.Command("git", "-C", root, "rev-parse", "HEAD").Output()
	if h.executable != "" {
		manifest["test_daemon_build_tag"] = ""
		manifest["production_release_assembly"] = true
	}
	if h.production {
		manifest["metrics_interval_ms"] = 60000
		manifest["log_interval_ms"] = 5000
	}
	manifest["source_head"] = strings.TrimSpace(string(head))
	manifest["transport"] = "local TCP through existing host transport seam"
	binaryHashes := map[string]string{}
	for _, name := range []string{"unic", "unicd"} {
		v, _ := os.ReadFile(filepath.Join(d, name+".sha256"))
		binaryHashes[name] = string(v)
	}
	manifest["binaries_sha256"] = binaryHashes
	if !manifest["source_unchanged"].(bool) {
		h.t.Error("source files changed during the acceptance run")
	}

	b, _ = json.MarshalIndent(manifest, "", "  ")
	_ = os.WriteFile(filepath.Join(d, "manifest.json"), b, 0600)
	b, _ = json.MarshalIndent(h.assertions, "", "  ")
	_ = os.WriteFile(filepath.Join(d, "assertions.json"), b, 0600)
	for _, dirname := range []string{"logs", "logs-saved"} {
		root := filepath.Join(h.home, dirname)
		st, err := os.Lstat(root)
		if err != nil || !st.IsDir() {
			continue
		}
		files, _ := os.ReadDir(root)
		var local []byte
		for _, file := range files {
			if file.Type().IsRegular() && strings.HasPrefix(file.Name(), "segment-") {
				data, err := os.ReadFile(filepath.Join(root, file.Name()))
				if err == nil {
					local = append(local, data...)
				}
			}
		}
		_ = os.WriteFile(filepath.Join(d, dirname+".jsonl"), local, 0600)
	}
	b, _ = json.MarshalIndent(h.resourceSamples, "", "  ")
	_ = os.WriteFile(filepath.Join(d, "resource-samples.json"), b, 0600)
	replay := `#!/bin/sh
set -eu
root=$(git rev-parse --show-toplevel)
cd "$root"
bin=$(mktemp -d /private/tmp/unic-otlp-replay.XXXXXX)
trap 'rm -rf "$bin"' EXIT
go build -race -tags unic_e2e -o "$bin/unicd" ./app/service/cmd/unicd
go build -race -o "$bin/unic" ./app/service/cmd/unic
TMPDIR=/private/tmp UNIC_E2E=1 UNIC_E2E_BIN="$bin" go test -race ./e2e/cli -run TestObservability -count=1 -v
`
	if h.posthog {
		replay = strings.ReplaceAll(replay, "-run TestObservability", "-run TestPostHog")
	}
	_ = os.WriteFile(filepath.Join(d, "replay.sh"), []byte(replay), 0700)
	b, _ = json.Marshal(map[string]any{"failed": h.t.Failed(), "requests": len(h.requests), "scenarios": h.scenarioResults})
	_ = os.WriteFile(filepath.Join(d, "summary.json"), b, 0600)
	if h.posthog {
		timeline := make([]map[string]any, 0, len(h.requests))
		var analytics []any
		for _, r := range h.requests {
			timeline = append(timeline, map[string]any{"path": r.Path, "at": r.At, "authorized": r.Authorized})
			if r.Path == "/batch/" {
				var batch struct {
					Batch []any `json:"batch"`
				}
				_ = json.Unmarshal(r.Body, &batch)
				analytics = append(analytics, batch.Batch...)
			}
		}
		b, _ = json.MarshalIndent(timeline, "", "  ")
		_ = os.WriteFile(filepath.Join(d, "requests.redacted.json"), b, 0600)
		b, _ = json.MarshalIndent(analytics, "", "  ")
		_ = os.WriteFile(filepath.Join(d, "analytics.decoded.json"), b, 0600)
		b, _ = json.MarshalIndent(map[string]any{"hosted": "NOT_RUN", "reason": "Cloud project and region not selected", "local_failed": h.t.Failed()}, "", "  ")
		_ = os.WriteFile(filepath.Join(d, "hosted-receipt.json"), b, 0600)
		exit := 0
		if h.t.Failed() {
			exit = 1
		}
		b, _ = json.MarshalIndent(map[string]any{"replay": "./replay.sh", "build_tag": "unic_e2e", "test_selector": "^TestPostHog", "exit_code": exit}, "", "  ")
		_ = os.WriteFile(filepath.Join(d, "commands.json"), b, 0600)
		entries, _ := os.ReadDir(d)
		var sums strings.Builder
		for _, entry := range entries {
			if entry.IsDir() || entry.Name() == "SHA256SUMS" {
				continue
			}
			data, e := os.ReadFile(filepath.Join(d, entry.Name()))
			if e != nil {
				continue
			}
			sum := sha256.Sum256(data)
			fmt.Fprintf(&sums, "%x  %s\n", sum, entry.Name())
		}
		_ = os.WriteFile(filepath.Join(d, "SHA256SUMS"), []byte(sums.String()), 0600)
	}
}

func (h *observabilityRun) hasLogSince(event string, since time.Time) bool {
	h.mu.Lock()
	defer h.mu.Unlock()
	for _, r := range h.requests {
		if r.Path != "/v1/logs" || r.At.Before(since) {
			continue
		}
		var p collectlogs.ExportLogsServiceRequest
		if proto.Unmarshal(r.Body, &p) != nil {
			continue
		}
		for _, rs := range p.ResourceLogs {
			for _, ss := range rs.ScopeLogs {
				for _, record := range ss.LogRecords {
					if record.Body.GetStringValue() == event {
						return true
					}
				}
			}
		}
	}
	return false
}
func (h *observabilityRun) metricCount(stage, result string) int64 {
	h.mu.Lock()
	defer h.mu.Unlock()
	var count int64
	for _, r := range h.requests {
		if r.Path != "/v1/metrics" {
			continue
		}
		var p collectmetrics.ExportMetricsServiceRequest
		if proto.Unmarshal(r.Body, &p) != nil {
			continue
		}
		for _, rs := range p.ResourceMetrics {
			for _, ss := range rs.ScopeMetrics {
				for _, m := range ss.Metrics {
					if m.Name != "unic.pipeline.operations" {
						continue
					}
					for _, point := range m.GetSum().GetDataPoints() {
						values := map[string]string{}
						for _, a := range point.Attributes {
							values[a.Key] = a.Value.GetStringValue()
						}
						if values["stage"] == stage && values["result"] == result && point.GetAsInt() > count {
							count = point.GetAsInt()
						}
					}
				}
			}
		}
	}
	return count
}
func (h *observabilityRun) signalDrops(t *testing.T, signal string) uint64 {
	var st struct {
		Logs struct {
			Dropped uint64 `json:"dropped_records"`
		} `json:"logs"`
		Metrics struct {
			Dropped uint64 `json:"dropped_records"`
		} `json:"metrics"`
	}
	if err := json.Unmarshal(h.get(t, "/v1/diagnostics/telemetry"), &st); err != nil {
		t.Fatal(err)
	}
	if signal == "logs" {
		return st.Logs.Dropped
	}
	return st.Metrics.Dropped
}
func (h *observabilityRun) assertOTLP(t *testing.T) {
	h.mu.Lock()
	defer h.mu.Unlock()
	allowed := []string{"host.arch", "os.type", "service.name", "service.version"}
	seenDuration := false
	for _, r := range h.requests {
		switch r.Path {
		case "/v1/metrics":
			var p collectmetrics.ExportMetricsServiceRequest
			if err := proto.Unmarshal(r.Body, &p); err != nil {
				t.Fatal(err)
			}
			for _, rs := range p.ResourceMetrics {
				var keys []string
				for _, a := range rs.Resource.Attributes {
					keys = append(keys, a.Key)
				}
				sort.Strings(keys)
				if strings.Join(keys, ",") != strings.Join(allowed, ",") {
					t.Fatalf("unsafe metric Resource: %v", keys)
				}
				for _, ss := range rs.ScopeMetrics {
					for _, m := range ss.Metrics {
						if m.Name == "unic.pipeline.duration" {
							seenDuration = true
							if m.Unit != "s" {
								t.Fatal("wrong duration unit")
							}
							for _, d := range m.GetHistogram().GetDataPoints() {
								if len(d.ExplicitBounds) != 14 || d.Sum == nil || *d.Sum < 0 {
									t.Fatal("invalid histogram")
								}
							}
						}
					}
				}
			}
		case "/v1/logs":
			var p collectlogs.ExportLogsServiceRequest
			if err := proto.Unmarshal(r.Body, &p); err != nil {
				t.Fatal(err)
			}
			for _, rs := range p.ResourceLogs {
				var keys []string
				for _, a := range rs.Resource.Attributes {
					keys = append(keys, a.Key)
				}
				sort.Strings(keys)
				if strings.Join(keys, ",") != strings.Join(allowed, ",") {
					t.Fatalf("unsafe log Resource: %v", keys)
				}
			}
		}
	}
	if !seenDuration {
		t.Fatal("no duration metric")
	}
}

func TestObservabilityPipeline(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	a := newObservabilityRun(t)
	b := newObservabilityRun(t)
	a.artifactName = "pipeline-alpha"
	b.artifactName = "pipeline-beta"
	a.start(t, a.server.URL)
	b.start(t, b.server.URL)
	for _, h := range []*observabilityRun{a, b} {
		h.cli(t, "config", "set", "telemetry.enabled", "true")
	}
	code, body := a.request(t, "POST", "/v1/pairing-ticket", "")
	if code != 200 {
		t.Fatalf("ticket export failed: %d %s", code, body)
	}
	var ticketResult struct {
		Ticket string `json:"ticket"`
	}
	if err := json.Unmarshal(body, &ticketResult); err != nil {
		t.Fatal(err)
	}
	ticket := ticketResult.Ticket
	b.cli(t, "pair", ticket)
	for _, h := range []*observabilityRun{a, b} {
		var pending []struct {
			ID          string `json:"id"`
			Fingerprint string `json:"fingerprint"`
		}
		h.eventually(t, func() bool { _ = json.Unmarshal(h.get(t, "/v1/pending"), &pending); return len(pending) > 0 })
		if pending[0].Fingerprint == "" {
			t.Fatal("pairing has no fingerprint")
		}
		h.cli(t, "approve", pending[0].ID, pending[0].Fingerprint)
	}
	for _, h := range []*observabilityRun{a, b} {
		h.eventually(t, func() bool {
			var st struct {
				Peers []struct {
					Connected bool `json:"connected"`
				} `json:"peers"`
			}
			_ = json.Unmarshal(h.get(t, "/v1/status"), &st)
			return len(st.Peers) > 0 && st.Peers[0].Connected
		})
	}
	a.cli(t, "copy", "pipeline-private-synthetic-content")
	b.eventually(t, func() bool {
		code, body := b.request(t, "GET", "/v1/clipboard", "")
		return code == 200 && strings.Contains(string(body), "pipeline-private-synthetic-content")
	})
	a.eventually(t, func() bool {
		return a.metricCount("commit", "succeeded") == 1 && a.metricCount("send", "succeeded") >= 1
	})
	b.eventually(t, func() bool {
		return b.metricCount("receive", "succeeded") == 1 && b.metricCount("apply", "succeeded") == 1
	})
	if a.metricCount("receive", "succeeded") != 0 {
		t.Fatal("application echoed to sender")
	}
	a.assertOTLP(t)
	b.assertOTLP(t)
}

func (h *observabilityRun) run(t *testing.T, name string, fn func(*testing.T)) {
	if t.Failed() {
		return
	}
	passed := t.Run(name, fn)
	status := "passed"
	if !passed {
		status = "failed"
	}
	h.scenarioResults[name] = status
	h.assertions = append(h.assertions, map[string]any{"scenario": name, "expected": "passed", "actual": status})
}
func observabilitySourceHashes(t *testing.T) map[string]string {
	t.Helper()
	root, err := repoRoot()
	if err != nil {
		t.Fatal(err)
	}
	cmd := exec.Command("git", "-C", root, "ls-files", "--cached", "--others", "--exclude-standard", "--", "go.mod", "go.sum", "engine", "internal", "app/service", "app/desktop", "e2e/cli")
	out, err := cmd.Output()
	if err != nil {
		t.Fatal(err)
	}
	hashes := map[string]string{}
	for _, path := range strings.Split(strings.TrimSpace(string(out)), "\n") {
		if path == "" {
			continue
		}
		b, err := os.ReadFile(filepath.Join(root, path))
		if err != nil {
			t.Fatal(err)
		}
		sum := sha256.Sum256(b)
		hashes[path] = hex.EncodeToString(sum[:])
	}
	return hashes
}
func sameHashes(a, b map[string]string) bool {
	if len(a) != len(b) {
		return false
	}
	for k, v := range a {
		if b[k] != v {
			return false
		}
	}
	return true
}

func (h *observabilityRun) sampleResources(t *testing.T, label string) {
	var sample map[string]any
	if err := json.Unmarshal(h.get(t, "/test/resources"), &sample); err != nil {
		t.Fatal(err)
	}
	sample["phase"] = label
	sample["at"] = time.Now().UTC()
	h.resourceSamples = append(h.resourceSamples, sample)
	if len(h.resourceSamples) > 1 {
		before := h.resourceSamples[0]["goroutines"].(float64)
		now := sample["goroutines"].(float64)
		if now > before+12 {
			t.Fatalf("goroutines grew across 100 signal generations: %.0f -> %.0f", before, now)
		}
	}
}

func TestObservabilityProductionCycle(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	h := newObservabilityRun(t)
	h.artifactName = "production-cycle"
	h.production = true
	h.start(t, h.server.URL)
	// Observe a full production export/timeout window with both OTEL endpoint
	// environment variables set. No signal is allowed to initialize while off.
	h.requireNoRequests(t, 64*time.Second)
	enabled := time.Now()
	h.cli(t, "config", "set", "telemetry.enabled", "true")
	h.cli(t, "copy", "production-cycle-private-fixture")
	h.eventually(t, func() bool { return h.hasPath("/v1/metrics") })
	h.mu.Lock()
	var first time.Time
	for _, r := range h.requests {
		if r.Path == "/v1/metrics" {
			first = r.At
			break
		}
	}
	h.mu.Unlock()
	if first.Sub(enabled) < 55*time.Second {
		t.Fatal("production metrics interval was shortened")
	}
	h.assertOTLP(t)
}

func TestObservabilityResources(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	for _, mode := range []string{"off", "normal", "debug", "pressure"} {
		for repeat := 0; repeat < 3; repeat++ {
			t.Run(fmt.Sprintf("%s-%d", mode, repeat), func(t *testing.T) {
				h := newObservabilityRun(t)
				h.artifactName = fmt.Sprintf("resources-%s-%d", mode, repeat)
				h.start(t, h.server.URL)
				if mode != "off" {
					h.cli(t, "config", "set", "telemetry.enabled", "true")
				}
				if mode == "debug" || mode == "pressure" {
					h.cli(t, "logs", "level", "debug", "--for", "15m")
					h.cli(t, "config", "set", "telemetry.logs.enabled", "true")
					h.cli(t, "logs", "level", "debug", "--target", "remote", "--for", "15m")
				}
				if mode == "pressure" {
					h.mu.Lock()
					h.status = 503
					h.mu.Unlock()
					code, _ := h.request(t, "POST", "/test/pressure", "")
					if code != 200 {
						t.Fatal("pressure fixture failed")
					}
				}
				for i := 0; i < 5; i++ {
					out, err := exec.Command("ps", "-p", fmt.Sprint(h.cmd.Process.Pid), "-o", "%cpu=,rss=").Output()
					if err != nil {
						t.Fatal(err)
					}
					h.resourceSamples = append(h.resourceSamples, map[string]any{"phase": "idle", "at": time.Now().UTC(), "ps_cpu_percent_rss_kib": strings.TrimSpace(string(out))})
					time.Sleep(100 * time.Millisecond)
				}
				var latencies []int64
				for i := 0; i < 50; i++ {
					start := time.Now()
					code, _ := h.request(t, "POST", "/test/commit", "")
					if code != 200 {
						t.Fatal("commit failed under telemetry load")
					}
					latencies = append(latencies, time.Since(start).Microseconds())
				}
				sort.Slice(latencies, func(i, j int) bool { return latencies[i] < latencies[j] })
				h.resourceSamples = append(h.resourceSamples, map[string]any{"phase": "commit", "sample_count": len(latencies), "latency_us": latencies, "p50_us": latencies[24], "p95_us": latencies[47], "p99_us": latencies[49], "wakeups": "not_collected", "sync_latency": "not_collected"})
				if mode == "pressure" && h.signalDrops(t, "logs") == 0 {
					t.Fatal("pressure mode did not saturate queue")
				}
			})
		}
	}
}

func TestObservabilityFaults(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	for _, scenario := range []struct {
		name      string
		status    int
		delay     time.Duration
		untrusted bool
		refused   bool
	}{
		{name: "unauthorized", status: 401}, {name: "rate-limited", status: 429}, {name: "unavailable", status: 503}, {name: "timeout", status: 200, delay: 5 * time.Second}, {name: "tls", status: 200, untrusted: true}, {name: "connection-refused", refused: true},
	} {
		t.Run(scenario.name, func(t *testing.T) {
			h := newObservabilityRun(t)
			h.artifactName = "fault-" + scenario.name
			h.status = scenario.status
			h.responseDelay = scenario.delay
			h.untrusted = scenario.untrusted
			endpoint := h.server.URL
			if scenario.refused {
				endpoint = "https://127.0.0.1:1"
			}
			h.start(t, endpoint)
			h.cli(t, "config", "set", "telemetry.enabled", "true")
			h.cli(t, "copy", "fault-private-fixture")
			h.eventually(t, func() bool { return strings.Contains(string(h.get(t, "/v1/diagnostics/telemetry")), `"degraded"`) })
			expected := scenario.name
			switch expected {
			case "connection-refused", "unavailable":
				expected = "unavailable"
			case "rate-limited":
				expected = "rate_limited"
			}
			h.contains(t, "/v1/diagnostics/telemetry", `"last_error_code":"`+expected+`"`)
			code, _ := h.request(t, "POST", "/test/commit", "")
			if code != 200 {
				t.Fatal("business failed because telemetry failed")
			}
			disabled := time.Now()
			h.cli(t, "config", "set", "telemetry.enabled", "false")
			if time.Since(disabled) > time.Second {
				t.Fatal("disable waited for exporter timeout")
			}
			h.requireNoRequests(t, 700*time.Millisecond)
		})
	}
}

package e2e

import (
	"bytes"
	"crypto/tls"
	"encoding/json"
	"encoding/pem"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"testing"
	"time"

	collectmetrics "go.opentelemetry.io/proto/otlp/collector/metrics/v1"
	"google.golang.org/protobuf/encoding/protojson"
	"google.golang.org/protobuf/proto"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopprocess"
	api "github.com/uniclipboard/unic/internal/localapi"
)

func scenarioResourceTelemetry(t *testing.T) {
	root, err := filepath.Abs("../../..")
	must(t, err)
	private := os.Getenv("UNIC_DESKTOP_E2E_CASE_PRIVATE")
	if private == "" {
		private = t.TempDir()
	}
	must(t, os.MkdirAll(private, 0700))

	base, err := os.MkdirTemp("", "uc-res-")
	must(t, err)
	base, err = filepath.EvalSymlinks(base)
	must(t, err)
	defer func() { _ = os.RemoveAll(base) }()

	// Local TLS OTLP receiver.
	var mu sync.Mutex
	roleSamples := map[string]int{}
	var decodedReqs []json.RawMessage
	lastMetricAt := time.Time{}
	srv := httptest.NewUnstartedServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		body, _ := io.ReadAll(r.Body)
		if r.URL.Path == "/v1/metrics" {
			var p collectmetrics.ExportMetricsServiceRequest
			if proto.Unmarshal(body, &p) == nil {
				if raw, merr := (protojson.MarshalOptions{}).Marshal(&p); merr == nil {
					mu.Lock()
					decodedReqs = append(decodedReqs, json.RawMessage(raw))
					mu.Unlock()
				}
				mu.Lock()
				lastMetricAt = time.Now()
				for _, rs := range p.ResourceMetrics {
					for _, ss := range rs.ScopeMetrics {
						for _, m := range ss.Metrics {
							if m.Name == "unic.resource.window.time" {
								for _, dp := range m.GetSum().GetDataPoints() {
									for _, a := range dp.Attributes {
										if a.Key == "process_role" {
											roleSamples[a.Value.GetStringValue()]++
										}
									}
								}
							}
						}
					}
				}
				mu.Unlock()
			}
		}
		w.WriteHeader(200)
	}))
	srv.TLS = &tls.Config{MinVersion: tls.VersionTLS12}
	srv.StartTLS()
	defer srv.Close()

	caPath := filepath.Join(base, "ca.pem")
	must(t, os.WriteFile(caPath, pem.EncodeToMemory(&pem.Block{Type: "CERTIFICATE", Bytes: srv.Certificate().Raw}), 0600))

	bin := filepath.Join(private, "bin")
	must(t, os.MkdirAll(bin, 0700))
	suffix := ""
	if runtime.GOOS == "windows" {
		suffix = ".exe"
	}
	for _, spec := range []struct {
		name string
		tags string
		pkg  string
	}{
		{"unic", "", "./app/service/cmd/unic"},
		{"unicd", "unic_e2e", "./app/service/cmd/unicd"},
		{"unic-desktop", "desktop_e2e", "./app/desktop/cmd/unic-desktop"},
	} {
		args := []string{"build", "-race"}
		if spec.tags != "" {
			args = append(args, "-tags", spec.tags)
		}
		args = append(args, "-o", filepath.Join(bin, spec.name+suffix), spec.pkg)
		cmd := exec.Command("go", args...)
		cmd.Dir = root
		out, berr := cmd.CombinedOutput()
		if berr != nil {
			t.Fatalf("build %s: %v\n%s", spec.name, berr, out)
		}
	}

	home := filepath.Join(base, "home")
	sock := filepath.Join(home, "unic.sock")
	discovery := filepath.Join(base, "discovery")
	must(t, os.MkdirAll(home, 0700))
	must(t, os.MkdirAll(filepath.Join(home, "board"), 0700))
	must(t, os.MkdirAll(discovery, 0700))
	must(t, os.WriteFile(filepath.Join(home, "e2e-owner"), []byte("desktop-e2e"), 0600))
	must(t, os.WriteFile(filepath.Join(home, "desktop-setup.json"), []byte(`{"complete":true}`), 0600))
	cfgTOML := fmt.Sprintf("[desktop]\nmode = \"normal\"\nstart_in_background = true\nquick_panel_shortcut = \"\"\n[telemetry]\nenabled = true\nendpoint = %q\n[sync]\nenabled = false\n", srv.URL)
	must(t, os.WriteFile(filepath.Join(home, "config.toml"), []byte(cfgTOML), 0600))

	var env []string
	for _, entry := range os.Environ() {
		if !strings.HasPrefix(entry, "UNIC_") && !strings.HasPrefix(entry, "MYGO_") {
			env = append(env, entry)
		}
	}
	env = append(env,
		"UNIC_HOME="+home,
		"UNIC_SOCK="+sock,
		"UNIC_DESKTOP_E2E_PROFILE="+home,
		"MYGO_AUTOMATION=",
		"SSL_CERT_FILE="+caPath,
		"UNIC_E2E_FAST_DIAGNOSTICS=1",
		"UNIC_E2E_DIAGNOSTICS=1",
		"UNIC_DESKTOP_E2E_PROCESS_FIXTURE=1",
		"UNIC_DESKTOP_E2E_RESOURCE_INTERVAL=300ms",
		"UNIC_RENDEZVOUS_URL=http://127.0.0.1:1",
		"HOME="+discovery,
		"USERPROFILE="+discovery,
		"APPDATA="+filepath.Join(discovery, "roaming"),
		"LOCALAPPDATA="+filepath.Join(discovery, "local"),
		"XDG_CONFIG_HOME="+filepath.Join(discovery, "config"),
		"XDG_DATA_HOME="+filepath.Join(discovery, "data"),
	)

	cli := func(args ...string) {
		t.Helper()
		cmd := exec.Command(filepath.Join(bin, "unic"+suffix), args...)
		cmd.Env = env
		out, err := cmd.CombinedOutput()
		if err != nil {
			t.Fatalf("unic %v: %v\n%s", args, err, out)
		}
	}
	cli("init", "--fresh", "--json")

	// Start tray (which starts unicd in normal mode), GUI, and Quick Panel.
	desktopBin := filepath.Join(bin, "unic-desktop"+suffix)
	var trayLogs bytes.Buffer
	trayCmd := exec.Command(desktopBin, "--role", "tray", "--mode", "normal", "--background")
	trayCmd.Env = env
	trayCmd.Stdout, trayCmd.Stderr = &trayLogs, &trayLogs
	must(t, trayCmd.Start())
	defer func() {
		_ = trayCmd.Process.Kill()
		_ = trayCmd.Wait()
	}()

	client := api.NewClient(sock)
	deadline := time.Now().Add(25 * time.Second)
	for {
		if _, err := client.Service(t.Context()); err == nil {
			break
		}
		if time.Now().After(deadline) {
			t.Fatalf("timed out waiting for unicd socket; tray output:\n%s", trayLogs.String())
		}
		time.Sleep(50 * time.Millisecond)
	}

	guiCmd := exec.Command(desktopBin, "--role", "gui")
	guiCmd.Env = env
	must(t, guiCmd.Start())
	defer func() {
		_ = guiCmd.Process.Kill()
		_ = guiCmd.Wait()
	}()

	panelCmd := exec.Command(desktopBin, "--role", "quick-panel")
	panelCmd.Env = env
	must(t, panelCmd.Start())
	defer func() {
		_ = panelCmd.Process.Kill()
		_ = panelCmd.Wait()
	}()

	// Verify all four roles appear in OTLP metrics via unicd.
	deadline = time.Now().Add(20 * time.Second)
	for {
		mu.Lock()
		ok := roleSamples["service"] > 0 && roleSamples["tray"] > 0 && roleSamples["gui"] > 0 && roleSamples["quick_panel"] > 0
		mu.Unlock()
		if ok {
			break
		}
		if time.Now().After(deadline) {
			mu.Lock()
			t.Fatalf("missing role samples before timeout: %+v", roleSamples)
		}
		time.Sleep(100 * time.Millisecond)
	}

	// Disable metrics and confirm export stops.
	cli("config", "set", "telemetry.metrics.enabled", "false")
	time.Sleep(600 * time.Millisecond)
	mu.Lock()
	disabledMark := time.Now()
	lastBefore := lastMetricAt
	mu.Unlock()
	time.Sleep(1000 * time.Millisecond)
	mu.Lock()
	lastAfter := lastMetricAt
	finalSamples := map[string]int{}
	for k, v := range roleSamples {
		finalSamples[k] = v
	}
	mu.Unlock()
	if !lastAfter.Equal(lastBefore) && lastAfter.After(disabledMark) {
		t.Fatalf("OTLP metrics continued after telemetry.metrics.enabled=false")
	}

	// Re-enable metrics and confirm all four roles rebuild fresh baselines cleanly.
	cli("config", "set", "telemetry.metrics.enabled", "true")
	deadline = time.Now().Add(20 * time.Second)
	for {
		mu.Lock()
		resumed := roleSamples["service"] > finalSamples["service"] &&
			roleSamples["tray"] > finalSamples["tray"] &&
			roleSamples["gui"] > finalSamples["gui"] &&
			roleSamples["quick_panel"] > finalSamples["quick_panel"]
		mu.Unlock()
		if resumed {
			break
		}
		if time.Now().After(deadline) {
			mu.Lock()
			t.Fatalf("roles did not resume after re-enabling metrics: paused=%+v current=%+v", finalSamples, roleSamples)
		}
		time.Sleep(100 * time.Millisecond)
	}

	mu.Lock()
	resumedSamples := map[string]int{}
	for k, v := range roleSamples {
		resumedSamples[k] = v
	}
	decodedCopy := append([]json.RawMessage(nil), decodedReqs...)
	mu.Unlock()

	_ = desktopprocess.Call(t.Context(), home, desktopprocess.Tray, "quit", nil, nil)
	if decodedBytes, merr := json.MarshalIndent(decodedCopy, "", "  "); merr == nil {
		must(t, os.WriteFile(filepath.Join(private, "otlp-decoded.json"), decodedBytes, 0600))
	}
	raw, _ := json.MarshalIndent(map[string]any{
		"status":               "passed",
		"paused_role_samples":  finalSamples,
		"resumed_role_samples": resumedSamples,
	}, "", "  ")
	must(t, os.WriteFile(filepath.Join(private, "manifest.json"), raw, 0600))
}

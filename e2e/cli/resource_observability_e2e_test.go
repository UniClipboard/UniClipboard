package e2e

import (
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"sort"
	"strings"
	"testing"
	"time"

	collectmetrics "go.opentelemetry.io/proto/otlp/collector/metrics/v1"
	commonpb "go.opentelemetry.io/proto/otlp/common/v1"
	"google.golang.org/protobuf/proto"
)

type resourceRoleSummary struct {
	CPUSeconds      float64 `json:"cpu_seconds"`
	WindowSeconds   float64 `json:"window_seconds"`
	UtilCount       uint64  `json:"util_count"`
	MemoryCount     uint64  `json:"memory_count"`
	GoHeapCount     uint64  `json:"go_heap_count"`
	Wakeups         int64   `json:"wakeups"`
	ContextSwitches int64   `json:"context_switches"`
}

// TestResourceObservability verifies process-level resource & power telemetry
// (0037) across unicd self-sampling, /v1/diagnostics/resource-sample validation,
// Delta aggregation, disable/re-enable barriers, privacy, and cli-temporary
// exclusion using real unic/unicd processes and a local TLS OTLP receiver.
func TestResourceObservability(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}

	h := newObservabilityRun(t)
	h.posthog = true
	h.artifactName = "resource-0037"
	cfg := fmt.Sprintf("[telemetry]\nendpoint=%q\n", h.server.URL+"/i")

	collectSummary := func() (map[string]*resourceRoleSummary, map[string]uint64) {
		h.mu.Lock()
		defer h.mu.Unlock()
		byRole := map[string]*resourceRoleSummary{}
		getRole := func(role string) *resourceRoleSummary {
			s, ok := byRole[role]
			if !ok {
				s = &resourceRoleSummary{}
				byRole[role] = s
			}
			return s
		}
		probeCounts := map[string]uint64{}
		allowed := []string{"host.arch", "os.type", "service.name", "service.version"}
		for _, r := range h.requests {
			if r.Path != "/i/v1/metrics" && r.Path != "/v1/metrics" {
				continue
			}
			var p collectmetrics.ExportMetricsServiceRequest
			if proto.Unmarshal(r.Body, &p) != nil {
				continue
			}
			for _, rs := range p.ResourceMetrics {
				var keys []string
				for _, a := range rs.Resource.Attributes {
					keys = append(keys, a.Key)
				}
				sort.Strings(keys)
				if strings.Join(keys, ",") != strings.Join(allowed, ",") {
					t.Fatalf("unsafe metric Resource keys: %v", keys)
				}
				for _, ss := range rs.ScopeMetrics {
					for _, m := range ss.Metrics {
						switch m.Name {
						case "unic.resource.cpu.time":
							for _, dp := range m.GetSum().GetDataPoints() {
								role := resourceAttrVal(dp.Attributes, "process_role")
								getRole(role).CPUSeconds += dp.GetAsDouble()
							}
						case "unic.resource.window.time":
							for _, dp := range m.GetSum().GetDataPoints() {
								role := resourceAttrVal(dp.Attributes, "process_role")
								getRole(role).WindowSeconds += dp.GetAsDouble()
							}
						case "unic.resource.cpu.utilization":
							for _, dp := range m.GetHistogram().GetDataPoints() {
								role := resourceAttrVal(dp.Attributes, "process_role")
								getRole(role).UtilCount += dp.Count
							}
						case "unic.resource.memory.footprint":
							for _, dp := range m.GetHistogram().GetDataPoints() {
								role := resourceAttrVal(dp.Attributes, "process_role")
								getRole(role).MemoryCount += dp.Count
							}
						case "unic.resource.memory.go_heap":
							for _, dp := range m.GetHistogram().GetDataPoints() {
								role := resourceAttrVal(dp.Attributes, "process_role")
								getRole(role).GoHeapCount += dp.Count
							}
						case "unic.power.wakeups":
							for _, dp := range m.GetSum().GetDataPoints() {
								role := resourceAttrVal(dp.Attributes, "process_role")
								getRole(role).Wakeups += dp.GetAsInt()
							}
						case "unic.power.context_switches":
							for _, dp := range m.GetSum().GetDataPoints() {
								role := resourceAttrVal(dp.Attributes, "process_role")
								getRole(role).ContextSwitches += dp.GetAsInt()
							}
						case "unic.resource.probe_status":
							for _, dp := range m.GetSum().GetDataPoints() {
								k := resourceAttrVal(dp.Attributes, "process_role") + ":" + resourceAttrVal(dp.Attributes, "probe") + ":" + resourceAttrVal(dp.Attributes, "status")
								probeCounts[k] += uint64(dp.GetAsInt())
							}
						}
					}
				}
			}
		}
		return byRole, probeCounts
	}

	h.run(t, "service-self-sampling", func(t *testing.T) {
		h.startConfig(t, cfg)
		h.eventually(t, func() bool {
			summary, _ := collectSummary()
			s := summary["service"]
			return s != nil && s.WindowSeconds > 0 && s.MemoryCount > 0 && s.GoHeapCount > 0
		})
		summary, _ := collectSummary()
		svc := summary["service"]
		if runtime.GOOS == "darwin" && svc.Wakeups < 0 {
			t.Fatalf("unexpected negative wakeups on darwin: %+v", svc)
		}
		if runtime.GOOS == "linux" && svc.ContextSwitches < 0 {
			t.Fatalf("unexpected negative context switches on linux: %+v", svc)
		}
	})

	h.run(t, "desktop-route-validation-and-bounds", func(t *testing.T) {
		invalidPayloads := []string{
			`{}`,
			`null`,
			`{"process_role":"service","app_state":"background","window_seconds":12,"cpu_seconds":0.5}`,
			`{"process_role":"tray","app_state":"background","window_seconds":12,"cpu_seconds":0.5,"extra":1}`,
			`{"process_role":"tray","process_role":"gui","app_state":"background","window_seconds":12,"cpu_seconds":0.5}`,
			`{"process_role":"tray","app_state":"background","window_seconds":0,"cpu_seconds":0.5}`,
			`{"process_role":"tray","app_state":"background","window_seconds":12,"cpu_seconds":-1}`,
			`{"process_role":"tray","app_state":"unknown_state","window_seconds":12,"cpu_seconds":0.5}`,
			`{"process_role":"tray","app_state":"background","window_seconds":12,"cpu_seconds":0.5,"pad":"` + strings.Repeat("a", 1025) + `"}`,
		}
		for _, body := range invalidPayloads {
			code, _ := h.request(t, "POST", "/v1/diagnostics/resource-sample", body)
			if code != 400 {
				t.Fatalf("expected 400 for invalid resource sample %q, got %d", body, code)
			}
		}

		// Valid short window (<10s) must NOT emit cpu.utilization; >=10s non-sleep MUST emit cpu.utilization.
		shortSample := `{"process_role":"gui","app_state":"foreground","window_seconds":4.0,"cpu_seconds":0.4,"memory_footprint_bytes":67108864,"memory_probe_status":"ok","go_heap_bytes":16777216}`
		if code, _ := h.request(t, "POST", "/v1/diagnostics/resource-sample", shortSample); code != 204 {
			t.Fatalf("expected 204 for valid short gui sample, got %d", code)
		}
		sleepJumpSample := `{"process_role":"gui","app_state":"foreground","window_seconds":15.0,"cpu_seconds":0.5,"sleep_jumped":true,"memory_footprint_bytes":67108864,"memory_probe_status":"ok","go_heap_bytes":16777216}`
		if code, _ := h.request(t, "POST", "/v1/diagnostics/resource-sample", sleepJumpSample); code != 204 {
			t.Fatalf("expected 204 for sleep-jumped gui sample, got %d", code)
		}
		time.Sleep(800 * time.Millisecond)
		s1, _ := collectSummary()
		if s1["gui"] != nil && s1["gui"].UtilCount != 0 {
			t.Fatalf("short or sleep-jumped sample must not emit cpu.utilization: %+v", s1["gui"])
		}

		validTray := `{"process_role":"tray","app_state":"background","window_seconds":12.0,"cpu_seconds":0.6,"memory_footprint_bytes":33554432,"memory_probe_status":"ok","go_heap_bytes":8388608,"wakeups":7,"wakeups_probe_status":"ok"}`
		if code, _ := h.request(t, "POST", "/v1/diagnostics/resource-sample", validTray); code != 204 {
			t.Fatalf("expected 204 for tray sample, got %d", code)
		}
		unsupportedPanel := `{"process_role":"quick_panel","app_state":"foreground","window_seconds":15.0,"cpu_seconds":0.3,"memory_footprint_bytes":25165824,"memory_probe_status":"ok","go_heap_bytes":8388608,"wakeups_probe_status":"unsupported"}`
		if code, _ := h.request(t, "POST", "/v1/diagnostics/resource-sample", unsupportedPanel); code != 204 {
			t.Fatalf("expected 204 for quick_panel sample, got %d", code)
		}
		h.eventually(t, func() bool {
			summary, probes := collectSummary()
			return summary["tray"] != nil && summary["tray"].UtilCount == 1 &&
				summary["quick_panel"] != nil && summary["quick_panel"].UtilCount == 1 &&
				probes["quick_panel:wakeups:unsupported"] == 1
		})
	})

	h.run(t, "disable-barrier-and-cli-temporary-exclusion", func(t *testing.T) {
		h.cli(t, "config", "set", "telemetry.metrics.enabled", "false")
		h.contains(t, "/v1/diagnostics/telemetry", `"metrics":{"requested_enabled":false,"state":"disabled"`)
		validTray := `{"process_role":"tray","app_state":"background","window_seconds":12.0,"cpu_seconds":0.6,"memory_footprint_bytes":33554432,"memory_probe_status":"ok","go_heap_bytes":8388608}`
		if code, _ := h.request(t, "POST", "/v1/diagnostics/resource-sample", validTray); code != 204 {
			t.Fatalf("expected 204 when metrics disabled, got %d", code)
		}
		h.requireNoRequests(t, 900*time.Millisecond)

		// Verify cli-temporary unicd skips resource sampling even with endpoint configured.
		tempHome := filepath.Join(filepath.Dir(h.home), "cli-temp-home")
		if err := os.MkdirAll(tempHome, 0700); err != nil {
			t.Fatal(err)
		}
		tempSock := filepath.Join(tempHome, "unic.sock")
		if err := os.WriteFile(filepath.Join(tempHome, "config.toml"), []byte(cfg), 0600); err != nil {
			t.Fatal(err)
		}
		h.mu.Lock()
		reqBefore := len(h.requests)
		h.mu.Unlock()
		runTempCLI := func(args ...string) {
			t.Helper()
			cmd := exec.Command(filepath.Join(binDir, "unic"), args...)
			cmd.Env = append(os.Environ(),
				"UNIC_HOME="+tempHome,
				"UNIC_SOCK="+tempSock,
				"SSL_CERT_FILE="+h.cert,
				"UNIC_E2E_DIAGNOSTICS=1",
				"UNIC_E2E_POSTHOG=true",
				"UNIC_E2E_FAST_DIAGNOSTICS=1",
			)
			out, err := cmd.CombinedOutput()
			if err != nil {
				t.Fatalf("cli-temporary %v failed: %v\n%s", args, err, out)
			}
		}
		runTempCLI("init", "--fresh", "--json")
		runTempCLI("dev", "diagnostics", "telemetry")
		time.Sleep(700 * time.Millisecond)
		h.mu.Lock()
		newReqs := append([]otlpRequest(nil), h.requests[reqBefore:]...)
		h.mu.Unlock()
		for _, r := range newReqs {
			if r.Path == "/i/v1/metrics" || r.Path == "/v1/metrics" {
				var p collectmetrics.ExportMetricsServiceRequest
				if proto.Unmarshal(r.Body, &p) == nil {
					for _, rs := range p.ResourceMetrics {
						for _, ss := range rs.ScopeMetrics {
							for _, m := range ss.Metrics {
								if strings.HasPrefix(m.Name, "unic.resource.") {
									t.Fatalf("cli-temporary daemon emitted resource metric %s", m.Name)
								}
							}
						}
					}
				}
			}
		}

		// Re-enable metrics and confirm fresh baseline resumes cleanly without replaying disabled-period data.
		beforeSummary, _ := collectSummary()
		beforeWin := beforeSummary["service"].WindowSeconds
		beforeMem := beforeSummary["service"].MemoryCount
		beforeHeap := beforeSummary["service"].GoHeapCount
		beforeTrayCPU := beforeSummary["tray"].CPUSeconds
		beforeTrayUtil := beforeSummary["tray"].UtilCount
		h.cli(t, "config", "set", "telemetry.metrics.enabled", "true")
		h.eventually(t, func() bool {
			cur, _ := collectSummary()
			svc := cur["service"]
			return svc != nil && svc.WindowSeconds > beforeWin && svc.MemoryCount > beforeMem && svc.GoHeapCount > beforeHeap
		})
		resumedTray := `{"process_role":"tray","app_state":"background","window_seconds":20.0,"cpu_seconds":4.2,"memory_footprint_bytes":33554432,"memory_probe_status":"ok","go_heap_bytes":8388608,"wakeups":3,"wakeups_probe_status":"ok"}`
		if code, _ := h.request(t, "POST", "/v1/diagnostics/resource-sample", resumedTray); code != 204 {
			t.Fatalf("expected 204 for resumed tray sample, got %d", code)
		}
		h.eventually(t, func() bool {
			cur, _ := collectSummary()
			tr := cur["tray"]
			return tr != nil && tr.UtilCount == beforeTrayUtil+1 && tr.CPUSeconds > beforeTrayCPU+4.0 && tr.CPUSeconds < beforeTrayCPU+4.5
		})
	})

	h.run(t, "privacy-and-summary-artifact", func(t *testing.T) {
		h.mu.Lock()
		for _, r := range h.requests {
			if strings.Contains(string(r.Body), h.home) {
				h.mu.Unlock()
				t.Fatal("home directory path leaked in telemetry")
			}
		}
		h.mu.Unlock()
		summary, probes := collectSummary()
		if artDir := os.Getenv("UNIC_E2E_ARTIFACTS"); artDir != "" {
			d := filepath.Join(artDir, h.artifactName)
			_ = os.MkdirAll(d, 0700)
			raw, _ := json.MarshalIndent(map[string]any{
				"roles":        summary,
				"probe_status": probes,
				"os":           runtime.GOOS,
				"arch":         runtime.GOARCH,
			}, "", "  ")
			_ = os.WriteFile(filepath.Join(d, "resource-summary.json"), raw, 0600)
		}
	})
}

func resourceServiceWin(fn func() (map[string]*resourceRoleSummary, map[string]uint64)) float64 {
	s, _ := fn()
	if svc := s["service"]; svc != nil {
		return svc.WindowSeconds
	}
	return 0
}

func resourceAttrVal(attrs []*commonpb.KeyValue, key string) string {
	for _, a := range attrs {
		if a.Key == key {
			return a.Value.GetStringValue()
		}
	}
	return ""
}

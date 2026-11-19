package e2e

import (
	"crypto/ed25519"
	"crypto/rand"
	"database/sql"
	"encoding/json"
	"fmt"
	"net"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"syscall"
	"testing"
	"time"

	"github.com/uniclipboard/unic/internal/identity"
	"github.com/uniclipboard/unic/internal/rendezvous"
	"github.com/uniclipboard/unic/internal/v1migration/v1fixture"
	"github.com/uniclipboard/unic/internal/wire"
)

// Failure inventory: docs/exec/observability-sync-followup.md. The controls
// inject TCP outages; all decisions and measurements use real service processes.
func TestSyncObservabilityRecoveryFollowup(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	for _, operation := range []string{"suspension", "revocation"} {
		t.Run(operation, func(t *testing.T) {
			a, b := newObservabilityRun(t), newObservabilityRun(t)
			a.artifactName, b.artifactName = "sync-followup-"+operation+"-alpha", "sync-followup-"+operation+"-beta"
			for _, h := range []*observabilityRun{a, b} {
				h.posthog = true
				syncFollowupStart(t, h, fmt.Sprintf("[telemetry]\nendpoint=%q\n", h.server.URL))
			}
			pairSyncServices(t, a, b)
			a.run(t, "establish-pending", func(t *testing.T) {
				for _, h := range []*observabilityRun{a, b} {
					syncControl(t, h, "dial-timeout")
				}
				syncControl(t, a, "disconnect")
				a.eventually(t, func() bool {
					return a.syncMetric("unic.sync.connection.errors", map[string]string{"phase": "dial", "reason": "timeout"}) > 0 && a.syncHistogramCount("unic.sync.recovery.pending", nil) > 0
				})
				if a.syncMetric("unic.sync.recovery.ends", nil) != 0 {
					t.Fatal("failed dial terminated pending episode")
				}
			})
			a.run(t, operation+"-terminates-once", func(t *testing.T) {
				result := "interrupted"
				if operation == "suspension" {
					if code, _ := a.request(t, "POST", "/test/runtime/suspended", ""); code != 200 {
						t.Fatal("runtime suspension failed")
					}
				} else {
					result = "abandoned"
					var status struct {
						Peers []struct {
							ID string `json:"id"`
						} `json:"peers"`
					}
					if err := json.Unmarshal(a.get(t, "/v1/status"), &status); err != nil || len(status.Peers) != 1 {
						t.Fatal("expected one paired device")
					}
					a.cli(t, "device", "remove", status.Peers[0].ID)
				}
				a.eventually(t, func() bool {
					return a.syncMetric("unic.sync.recovery.ends", map[string]string{"result": result}) == 1
				})
				before := a.syncHistogramCount("unic.sync.recovery.pending", nil)
				// The collector normally samples every ten seconds. An exported
				// cumulative count must remain unchanged for a complete period.
				time.Sleep(11 * time.Second)
				if a.syncHistogramCount("unic.sync.recovery.pending", nil) != before {
					t.Fatal("terminal episode still sampled as pending")
				}
				if a.syncMetric("unic.sync.recovery.ends", nil) != 1 {
					t.Fatal("episode terminated more than once")
				}
				a.assertions = append(a.assertions, map[string]any{"scenario": operation + "-episode", "terminal_result": result,
					"expected_terminal_count": 1, "actual_terminal_count": a.syncMetric("unic.sync.recovery.ends", nil),
					"pending_samples_before": before, "pending_samples_after": a.syncHistogramCount("unic.sync.recovery.pending", nil), "unchanged_window_seconds": 11})
			})
			a.run(t, "remove-fault-and-check-business", func(t *testing.T) {
				for _, h := range []*observabilityRun{a, b} {
					syncControl(t, h, "dial-normal")
				}
				if operation == "suspension" {
					if code, _ := a.request(t, "POST", "/test/runtime/active", ""); code != 200 {
						t.Fatal("runtime resume failed")
					}
					for _, h := range []*observabilityRun{a, b} {
						h.eventually(t, func() bool { return strings.Contains(string(h.get(t, "/v1/status")), `"connected":true`) })
					}
					a.cli(t, "dev", "copy", "0020-followup-after-resume")
					b.eventually(t, func() bool {
						code, body := b.request(t, "GET", "/v1/clipboard", "")
						return code == 200 && strings.Contains(string(body), "0020-followup-after-resume")
					})
				} else {
					var status struct {
						Peers []json.RawMessage `json:"peers"`
					}
					if json.Unmarshal(a.get(t, "/v1/status"), &status) != nil || len(status.Peers) != 0 {
						t.Fatal("revoked device reappeared")
					}
					a.cli(t, "dev", "copy", "0020-followup-after-revoke")
					time.Sleep(1500 * time.Millisecond)
					if b.eventCount("remote_content_applied") != 0 {
						t.Fatal("revoked device received content")
					}
				}
				if a.syncMetric("unic.sync.recovery.ends", map[string]string{"result": "recovered"}) != 0 {
					t.Fatal("terminal episode was later recovered")
				}
				a.eventually(t, func() bool { return a.metricCount("commit", "succeeded") == 1 })
				if operation == "suspension" {
					b.eventually(t, func() bool { return b.metricCount("receive", "succeeded") == 1 })
				}
				for _, h := range []*observabilityRun{a, b} {
					h.assertSyncPrivacy(t)
					if h == a || operation == "suspension" {
						h.assertOTLP(t)
					}
				}
			})
		})
	}
}

// The fixture obstructs only reserved files inside its new private profile.
// It never fills a disk, changes the host filesystem policy, or uses real data.
func TestSyncObservabilityDailyFollowup(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	for _, scenario := range []string{"malformed-startup", "invalid-day-startup", "symlink-startup", "temporary-directory-startup", "temporary-file-runtime", "rename-directory-runtime"} {
		t.Run(scenario, func(t *testing.T) {
			a, b := newObservabilityRun(t), newObservabilityRun(t)
			a.artifactName, b.artifactName = "sync-daily-"+scenario+"-alpha", "sync-daily-"+scenario+"-beta"
			must := func(err error) {
				t.Helper()
				if err != nil {
					t.Fatal(err)
				}
			}
			marker := filepath.Join(b.home, "analytics-remote-day")
			temp := filepath.Join(b.home, ".analytics-remote-day.tmp")
			protected := filepath.Join(b.dir, "protected-marker")
			switch scenario {
			case "malformed-startup":
				must(os.WriteFile(marker, []byte("0020-damaged-marker"), 0600))
			case "invalid-day-startup":
				must(os.WriteFile(marker, []byte(`{"identity":"0020-private-identity","day":"2026-99-99"}`), 0600))
			case "symlink-startup":
				must(os.WriteFile(protected, []byte("0020-protected-sentinel"), 0600))
				must(os.Symlink(protected, marker))
			case "temporary-directory-startup":
				must(os.Mkdir(temp, 0700))
				must(os.WriteFile(filepath.Join(temp, "keep"), []byte("0020-private-temp"), 0600))
			}
			for _, h := range []*observabilityRun{a, b} {
				h.posthog = true
				syncFollowupStart(t, h, fmt.Sprintf("[telemetry]\nendpoint=%q\n", h.server.URL))
			}
			pairSyncServices(t, a, b)
			before := syncFollowupAnalyticsDrops(t, b)
			switch scenario {
			case "temporary-file-runtime":
				must(os.WriteFile(temp, []byte("0020-occupied-temp"), 0600))
			case "rename-directory-runtime":
				must(os.Mkdir(marker, 0700))
				must(os.WriteFile(filepath.Join(marker, "keep"), []byte("0020-private-directory"), 0600))
			}
			b.run(t, "business-survives-marker-failure", func(t *testing.T) {
				for i := 0; i < 2; i++ {
					content := fmt.Sprintf("0020-daily-followup-%d", i)
					a.cli(t, "dev", "copy", content)
					b.eventually(t, func() bool {
						code, body := b.request(t, "GET", "/v1/clipboard", "")
						return code == 200 && strings.Contains(string(body), content)
					})
				}
				b.eventually(t, func() bool { return b.syncMetric("unic.sync.remote_applications", nil) == 2 })
				if strings.HasSuffix(scenario, "runtime") {
					b.eventually(t, func() bool { return syncFollowupAnalyticsDrops(t, b) == before+1 })
				} else if before < 1 {
					t.Fatal("marker load failure not counted locally")
				}
				time.Sleep(500 * time.Millisecond)
				if b.eventCount("remote_content_applied") != 0 {
					t.Fatal("failed marker admitted daily event")
				}
				b.assertions = append(b.assertions, map[string]any{"scenario": scenario, "expected_remote_applications": 2,
					"actual_remote_applications": b.syncMetric("unic.sync.remote_applications", nil),
					"expected_daily_events":      0, "actual_daily_events": b.eventCount("remote_content_applied"),
					"analytics_drops_before": before, "analytics_drops_after": syncFollowupAnalyticsDrops(t, b)})
				if scenario == "symlink-startup" {
					data, err := os.ReadFile(protected)
					if err != nil || string(data) != "0020-protected-sentinel" {
						t.Fatal("marker symlink modified protected data")
					}
				}
				b.assertSyncPrivacy(t)
				b.assertOTLP(t)
			})
		})
	}
}

func syncFollowupAnalyticsDrops(t *testing.T, h *observabilityRun) uint64 {
	t.Helper()
	var state struct {
		Analytics struct {
			Dropped uint64 `json:"dropped_records"`
		} `json:"analytics"`
	}
	if err := json.Unmarshal(h.get(t, "/v1/diagnostics/telemetry"), &state); err != nil {
		t.Fatal(err)
	}
	return state.Analytics.Dropped
}

// These are malicious wire inputs to the real service handshake. No diagnostic
// record, status reducer, identity verdict, or transport error is injected.
func TestSyncObservabilityHandshakeFollowup(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	a, b := newObservabilityRun(t), newObservabilityRun(t)
	a.artifactName, b.artifactName = "sync-handshake-alpha", "sync-handshake-beta"
	for _, h := range []*observabilityRun{a, b} {
		h.posthog = true
		syncFollowupStart(t, h, fmt.Sprintf("[telemetry]\nendpoint=%q\n", h.server.URL))
	}
	var offer struct {
		Ticket string `json:"ticket"`
	}
	code, body := a.request(t, "POST", "/v1/pairing-ticket", "")
	if code != 200 || json.Unmarshal(body, &offer) != nil {
		t.Fatal("pairing address unavailable")
	}
	ticket, ok := rendezvous.ParseText(offer.Ticket)
	if !ok {
		t.Fatal("pairing address decode failed")
	}
	for _, fault := range []string{"protocol_mismatch", "identity_mismatch"} {
		a.run(t, fault, func(t *testing.T) {
			c, err := net.DialTimeout("tcp", string(ticket.Access), time.Second)
			if err != nil {
				t.Fatal("private peer dial failed")
			}
			defer func() { _ = c.Close() }()
			_ = c.SetDeadline(time.Now().Add(3 * time.Second))
			w := wire.NewConn(c)
			if typ, _, err := w.ReadRaw(); err != nil || typ != "hello" {
				t.Fatal("service hello unavailable")
			}
			pk, _, err := ed25519.GenerateKey(rand.Reader)
			if err != nil {
				t.Fatal(err)
			}
			hello, err := identity.NewHello(pk)
			if err != nil {
				t.Fatal(err)
			}
			version := wire.Version
			if fault == "protocol_mismatch" {
				version++
			}
			if err := w.WriteJSON(wire.Hello{T: "hello", Version: version, H: hello.Encode(), Name: "0020-private-peer"}); err != nil {
				t.Fatal("synthetic hello write failed")
			}
			if fault == "identity_mismatch" {
				if typ, _, err := w.ReadRaw(); err != nil || typ != "proof" {
					t.Fatal("service proof unavailable")
				}
				if err := w.WriteJSON(wire.Proof{T: "proof", Sig: make([]byte, ed25519.SignatureSize)}); err != nil {
					t.Fatal("synthetic proof write failed")
				}
			}
			_ = c.Close()
			a.eventually(t, func() bool {
				return a.syncMetric("unic.sync.connection.errors", map[string]string{"phase": "identity_handshake", "reason": fault}) == 1
			})
			a.assertions = append(a.assertions, map[string]any{"scenario": "actual-handshake-fault", "reason": fault,
				"expected_errors": 1, "actual_errors": a.syncMetric("unic.sync.connection.errors", map[string]string{"phase": "identity_handshake", "reason": fault}),
				"diagnostic_record_injection": false})
		})
	}
	a.run(t, "legitimate-peer-after-rejected-handshakes", func(t *testing.T) {
		if a.syncMetric("unic.sync.session.ends", nil) != 0 {
			t.Fatal("rejected handshake created a session")
		}
		var page struct {
			Items []json.RawMessage `json:"items"`
		}
		if json.Unmarshal(a.get(t, "/v1/history"), &page) != nil || len(page.Items) != 0 {
			t.Fatal("rejected handshake created history")
		}
		pairSyncServices(t, a, b)
		a.cli(t, "dev", "copy", "0020-legitimate-after-handshake-faults")
		b.eventually(t, func() bool {
			code, body := b.request(t, "GET", "/v1/clipboard", "")
			return code == 200 && strings.Contains(string(body), "0020-legitimate-after-handshake-faults")
		})
		a.eventually(t, func() bool { return a.metricCount("commit", "succeeded") == 1 })
		b.eventually(t, func() bool { return b.metricCount("receive", "succeeded") == 1 })
		for _, h := range []*observabilityRun{a, b} {
			h.assertSyncPrivacy(t)
			h.assertOTLP(t)
		}
	})
}

func TestSyncObservabilityCrashFollowup(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	a, b := newObservabilityRun(t), newObservabilityRun(t)
	a.artifactName, b.artifactName = "sync-crash-alpha", "sync-crash-beta"
	for _, h := range []*observabilityRun{a, b} {
		h.posthog = true
		syncFollowupStart(t, h, fmt.Sprintf("[telemetry]\nendpoint=%q\n", h.server.URL))
	}
	pairSyncServices(t, a, b)
	a.run(t, "persist-daily-before-crash", func(t *testing.T) {
		a.cli(t, "dev", "copy", "0020-before-crash")
		b.eventually(t, func() bool { return b.eventCount("remote_content_applied") == 1 })
		if info, err := os.Lstat(filepath.Join(b.home, "analytics-remote-day")); err != nil || !info.Mode().IsRegular() {
			t.Fatal("daily admission was not persisted")
		}
	})
	a.run(t, "kill-pending-recovery-and-restart", func(t *testing.T) {
		for _, h := range []*observabilityRun{a, b} {
			syncControl(t, h, "dial-timeout")
		}
		syncControl(t, b, "disconnect")
		b.eventually(t, func() bool { return b.syncHistogramCount("unic.sync.recovery.pending", nil) > 0 })
		if err := b.cmd.Process.Signal(syscall.SIGKILL); err != nil {
			t.Fatal(err)
		}
		select {
		case err := <-b.done:
			if err == nil {
				t.Fatal("forced termination returned success")
			}
		case <-time.After(5 * time.Second):
			t.Fatal("forced termination did not exit")
		}
		b.cmd = nil
		b.mu.Lock()
		b.syncMetricStart = time.Now()
		b.mu.Unlock()
		syncFollowupStart(t, b, fmt.Sprintf("[telemetry]\nendpoint=%q\n", b.server.URL))
		syncControl(t, a, "dial-normal")
		for _, h := range []*observabilityRun{a, b} {
			h.eventually(t, func() bool { return strings.Contains(string(h.get(t, "/v1/status")), `"connected":true`) })
		}
		a.cli(t, "dev", "copy", "0020-after-crash")
		b.eventually(t, func() bool {
			code, body := b.request(t, "GET", "/v1/clipboard", "")
			return code == 200 && strings.Contains(string(body), "0020-after-crash")
		})
		b.eventually(t, func() bool { return b.syncMetric("unic.sync.remote_applications", nil) == 1 })
		if b.syncMetric("unic.sync.recovery.ends", nil) != 0 {
			t.Fatal("old pending episode was reconstructed after process crash")
		}
		time.Sleep(500 * time.Millisecond)
		if b.eventCount("remote_content_applied") != 1 {
			t.Fatal("crash replayed a persisted daily event")
		}
		b.assertions = append(b.assertions, map[string]any{"scenario": "crash-new-generation", "forced_signal": "SIGKILL",
			"expected_new_remote_applications": 1, "actual_new_remote_applications": b.syncMetric("unic.sync.remote_applications", nil),
			"expected_new_recovery_ends": 0, "actual_new_recovery_ends": b.syncMetric("unic.sync.recovery.ends", nil),
			"expected_total_daily_events": 1, "actual_total_daily_events": b.eventCount("remote_content_applied")})
		b.assertSyncPrivacy(t)
		b.assertOTLP(t)
	})
}

// A real offline v1 import and real CLI operations establish source categories.
// The only SQL write repairs the synthetic legacy fixture before importing it.
func TestSyncObservabilitySourceFollowup(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	h := newObservabilityRun(t)
	h.artifactName, h.posthog = "sync-source-followup", true
	source, dbpath, fixture, err := v1fixture.Build(filepath.Join(h.dir, "legacy-fixture"))
	if err != nil {
		t.Fatal(err)
	}
	db, err := sql.Open("sqlite", dbpath)
	if err != nil {
		t.Fatal(err)
	}
	_, err = db.Exec(`INSERT INTO clipboard_snapshot_representation VALUES('rep-bad','event-bad','text/plain','text/plain',?,NULL,9)`, []byte(fixture.Repaired))
	closeErr := db.Close()
	if err != nil || closeErr != nil {
		t.Fatal("legacy fixture repair failed")
	}
	cmd := exec.Command(filepath.Join(binDir, "unic"), "migrate", "--source", source, "--passphrase-stdin", "--json")
	cmd.Env, cmd.Stdin = h.env(), strings.NewReader(fixture.Passphrase+"\n")
	if out, err := cmd.CombinedOutput(); err != nil || !strings.Contains(string(out), `"status":"complete"`) {
		t.Fatal("explicit synthetic import failed")
	}
	syncFollowupStart(t, h, fmt.Sprintf("[telemetry]\nendpoint=%q\n[history]\nauto_cleanup=false\n", h.server.URL))
	h.run(t, "migration-source", func(t *testing.T) {
		var page struct {
			Items []struct {
				ID     string `json:"id"`
				Origin string `json:"origin"`
			} `json:"items"`
		}
		if err := json.Unmarshal(h.get(t, "/v1/history?origin=migration&kind=text"), &page); err != nil || len(page.Items) == 0 {
			t.Fatal("imported text unavailable through history API")
		}
		id := page.Items[0].ID
		h.cli(t, "history", "use", id, "--request-id", "0020-migration-source")
		h.cli(t, "history", "use", id, "--request-id", "0020-migration-source")
		h.eventually(t, func() bool { return syncFollowupSourceEvents(h, "migration") == 1 })
	})
	h.run(t, "local-and-action-source", func(t *testing.T) {
		h.cli(t, "dev", "copy", "0020-local-source")
		h.cli(t, "history", "use", "@1", "--request-id", "0020-local-source")
		h.eventually(t, func() bool { return syncFollowupSourceEvents(h, "local") == 1 })
		h.cli(t, "history", "action", "@1", "builtin.text/upper", "--request-id", "0020-action-source")
		h.cli(t, "history", "use", "@1", "--request-id", "0020-action-apply")
		h.cli(t, "history", "use", "@1", "--request-id", "0020-action-apply")
		h.eventually(t, func() bool { return syncFollowupSourceEvents(h, "action") == 1 })
		time.Sleep(500 * time.Millisecond)
		if h.eventCount("history_applied") != 3 || h.eventCount("remote_content_applied") != 0 {
			t.Fatal("explicit applications duplicated events or emitted automatic availability")
		}
		h.assertions = append(h.assertions, map[string]any{"scenario": "source-matrix", "expected_local": 1, "actual_local": syncFollowupSourceEvents(h, "local"),
			"expected_action": 1, "actual_action": syncFollowupSourceEvents(h, "action"), "expected_migration": 1, "actual_migration": syncFollowupSourceEvents(h, "migration"),
			"expected_total_history_events": 3, "actual_total_history_events": h.eventCount("history_applied"), "external": "NOT_RUN"})
		h.assertSyncPrivacy(t)
		h.assertOTLP(t)
	})
}

func syncFollowupSourceEvents(h *observabilityRun, source string) int {
	n := 0
	for _, event := range h.analytics() {
		properties, _ := event["properties"].(map[string]any)
		if event["event"] == "history_applied" && properties["source_kind"] == source {
			n++
		}
	}
	return n
}

func TestSyncObservabilityCostFollowup(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	for _, mode := range []string{"disabled", "normal", "debug", "queue-pressure"} {
		t.Run(mode, func(t *testing.T) {
			a, b := newObservabilityRun(t), newObservabilityRun(t)
			a.artifactName, b.artifactName = "sync-cost-"+mode+"-alpha", "sync-cost-"+mode+"-beta"
			for _, h := range []*observabilityRun{a, b} {
				h.posthog = true // Bind only the synthetic receiver credential; analytics stays disabled.
				level := "warn"
				if mode == "debug" || mode == "queue-pressure" {
					level = "debug"
				}
				syncFollowupStart(t, h, fmt.Sprintf("[telemetry]\nenabled=%t\nendpoint=%q\n[telemetry.logs]\nenabled=true\nlevel=%q\n[telemetry.analytics]\nenabled=false\n", mode != "disabled", h.server.URL, level))
			}
			pairSyncServices(t, a, b)
			time.Sleep(600 * time.Millisecond)
			for repeat := 0; repeat < 3; repeat++ {
				a.run(t, fmt.Sprintf("matched-workload-%d", repeat), func(t *testing.T) {
					if mode == "queue-pressure" {
						for _, h := range []*observabilityRun{a, b} {
							h.mu.Lock()
							h.status, h.responseDelay = 503, 3*time.Second
							h.mu.Unlock()
							if code, _ := h.request(t, "POST", "/test/pressure", ""); code != 200 {
								t.Fatal("queue pressure fixture failed")
							}
						}
					}
					beforeA, beforeB := syncFollowupResources(t, a), syncFollowupResources(t, b)
					beforeExportA, beforeExportB := syncFollowupExportCost(a), syncFollowupExportCost(b)
					start := time.Now()
					var submission, synchronization []int64
					for i := 0; i < 12; i++ {
						content := fmt.Sprintf("0020-cost-matched-%02d-%02d", repeat, i)
						began := time.Now()
						a.cli(t, "dev", "copy", content)
						submission = append(submission, time.Since(began).Microseconds())
						b.eventually(t, func() bool {
							code, body := b.request(t, "GET", "/v1/clipboard", "")
							return code == 200 && strings.Contains(string(body), content)
						})
						synchronization = append(synchronization, time.Since(began).Microseconds())
					}
					// Include one full accelerated metrics interval after the last
					// copy. Production-cadence acceptance is a separate scenario.
					time.Sleep(600 * time.Millisecond)
					afterA, afterB := syncFollowupResources(t, a), syncFollowupResources(t, b)
					a.resourceSamples = append(a.resourceSamples, map[string]any{
						"phase": "matched-sync-cost", "mode": mode, "repeat": repeat,
						"copies": 12, "window_us": time.Since(start).Microseconds(),
						"transport":     "real session/wire over injected local TCP; not Tailcat",
						"submission_us": submission, "synchronization_observed_us": synchronization,
						"submission_quantiles_us": syncFollowupQuantiles(submission), "synchronization_quantiles_us": syncFollowupQuantiles(synchronization),
						"sender_before": beforeA, "sender_after": afterA, "receiver_before": beforeB, "receiver_after": afterB,
						"sender_export_delta":   syncFollowupCostDelta(beforeExportA, syncFollowupExportCost(a)),
						"receiver_export_delta": syncFollowupCostDelta(beforeExportB, syncFollowupExportCost(b)),
						"sender_cpu_seconds":    afterA["cpu_seconds"].(float64) - beforeA["cpu_seconds"].(float64),
						"receiver_cpu_seconds":  afterB["cpu_seconds"].(float64) - beforeB["cpu_seconds"].(float64),
						"threshold_acceptance":  "NOT_RUN; matched observations only",
					})
					if mode == "disabled" && (syncFollowupExportCost(a)["requests"] != 0 || syncFollowupExportCost(b)["requests"] != 0) {
						t.Fatal("disabled telemetry sent requests")
					}
					if mode == "queue-pressure" && (a.signalDrops(t, "logs") == 0 || b.signalDrops(t, "logs") == 0) {
						t.Fatal("queue pressure did not record drops")
					}
				})
			}
		})
	}
}

func syncFollowupResources(t *testing.T, h *observabilityRun) map[string]any {
	t.Helper()
	var state map[string]any
	if err := json.Unmarshal(h.get(t, "/test/resources"), &state); err != nil {
		t.Fatal(err)
	}
	pid := h.cmd.Process.Pid
	state["cpu_seconds"], state["rss_kib"] = processCPU(t, pid), processRSS(t, pid)
	// Keep counts only. lsof path/name data never reaches the artifact or log.
	output, err := exec.Command("lsof", "-n", "-P", "-p", strconv.Itoa(pid), "-Ff").Output()
	if err != nil {
		t.Fatal("process descriptor measurement unavailable")
	}
	descriptors := 0
	for _, line := range strings.Split(string(output), "\n") {
		if len(line) > 1 && line[0] == 'f' && line[1] >= '0' && line[1] <= '9' {
			descriptors++
		}
	}
	state["open_descriptors"] = descriptors
	return state
}

func syncFollowupExportCost(h *observabilityRun) map[string]int64 {
	h.mu.Lock()
	defer h.mu.Unlock()
	cost := map[string]int64{"requests": int64(len(h.requests)), "body_bytes": 0,
		"tls_wire_received": h.wireReceived.Load(), "tls_wire_sent": h.wireSent.Load()}
	for _, request := range h.requests {
		cost["body_bytes"] += int64(len(request.Body))
	}
	return cost
}

func syncFollowupCostDelta(before, after map[string]int64) map[string]int64 {
	delta := make(map[string]int64, len(before))
	for key, value := range after {
		delta[key] = value - before[key]
	}
	return delta
}

func syncFollowupQuantiles(values []int64) map[string]int64 {
	sorted := append([]int64(nil), values...)
	sort.Slice(sorted, func(i, j int) bool { return sorted[i] < sorted[j] })
	return map[string]int64{"p50": sorted[(len(sorted)-1)*50/100], "p95": sorted[(len(sorted)-1)*95/100], "max": sorted[len(sorted)-1]}
}

func syncFollowupStart(t *testing.T, h *observabilityRun, cfg string) {
	t.Helper()
	h.startConfig(t, cfg)
	h.assertions = append(h.assertions, map[string]any{
		"scenario": "test-configuration", "config_toml_redacted": strings.ReplaceAll(cfg, h.server.URL, "https://local-tls-receiver.invalid"),
		"metrics_interval_ms": 500, "logs_interval_ms": 100, "transport": "injected local TCP; real session/wire", "system_clipboard": false,
	})
}

func TestSyncObservabilityExternalFollowup(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	t.Setenv("UNIC_E2E_SYNC_FOLLOWUP", "1")
	h := newObservabilityRun(t)
	h.artifactName, h.posthog = "sync-external-followup", true
	syncFollowupStart(t, h, fmt.Sprintf("[telemetry]\nendpoint=%q\n", h.server.URL))
	h.run(t, "external-receive-and-source", func(t *testing.T) {
		// Ready intentionally ignores pre-existing receipts. Complete a real adapter
		// read round trip before receiving fresh content. Capabilities precede ready.
		h.eventually(t, func() bool { code, _ := h.request(t, "GET", "/v1/clipboard", ""); return code == 200 })
		if code, _ := h.request(t, "POST", "/test/followup/external", ""); code != 200 {
			t.Fatal("external fixture receive failed")
		}
		h.eventually(t, func() bool {
			code, body := h.request(t, "GET", "/v1/clipboard", "")
			return code == 200 && strings.Contains(string(body), "0020-external-private-content")
		})
		h.eventually(t, func() bool {
			return h.metricCount("receive", "succeeded") == 1 && h.metricCount("apply", "succeeded") == 1
		})
		h.cli(t, "history", "use", "@1", "--request-id", "0020-external-history")
		h.cli(t, "history", "use", "@1", "--request-id", "0020-external-history")
		h.eventually(t, func() bool { return syncFollowupSourceEvents(h, "external") == 1 })
		if code, _ := h.request(t, "POST", "/test/followup/external", ""); code != 409 {
			t.Fatal("external duplicate was not declined")
		}
		h.eventually(t, func() bool { return h.metricCount("receive", "duplicate") == 1 })
		time.Sleep(600 * time.Millisecond)
		var page struct {
			Items []struct {
				Origin string `json:"origin"`
			} `json:"items"`
		}
		if json.Unmarshal(h.get(t, "/v1/history"), &page) != nil || len(page.Items) != 1 || page.Items[0].Origin != "external" {
			t.Fatal("external receive lost its origin or duplicated history")
		}
		if h.syncMetric("unic.sync.remote_applications", nil) != 0 || h.eventCount("remote_content_applied") != 0 || h.eventCount("history_applied") != 1 {
			t.Fatal("external duplicate or explicit application duplicated automatic availability")
		}
		h.assertions = append(h.assertions, map[string]any{"scenario": "external-real-receive", "diagnostic_record_injection": false,
			"expected_history_items": 1, "actual_history_items": len(page.Items), "expected_external_history_events": 1, "actual_external_history_events": syncFollowupSourceEvents(h, "external"),
			"expected_remote_applications": 0, "actual_remote_applications": h.syncMetric("unic.sync.remote_applications", nil), "expected_daily_events": 0, "actual_daily_events": h.eventCount("remote_content_applied")})
	})
	h.run(t, "plugin-observer-panic-isolation", func(t *testing.T) {
		h.cli(t, "dev", "copy", "0020-after-plugin-observer-panic")
		h.eventually(t, func() bool {
			var status struct {
				CommittedCallbacks uint64 `json:"committed_callbacks"`
			}
			return json.Unmarshal(h.get(t, "/test/followup/observer"), &status) == nil && status.CommittedCallbacks == 2
		})
		h.eventually(t, func() bool { return h.metricCount("commit", "succeeded") == 1 })
		code, body := h.request(t, "GET", "/v1/clipboard", "")
		if code != 200 || !strings.Contains(string(body), "0020-after-plugin-observer-panic") {
			t.Fatal("plugin observer panic blocked a fresh local application")
		}
		h.assertions = append(h.assertions, map[string]any{"scenario": "plugin-observer-panic", "callbacks_panicked": 2, "successful_external_receives": 1,
			"successful_local_commits": 1, "syncdiag_observer_panic": "NOT_RUN"})
		h.assertSyncPrivacy(t)
		h.assertOTLP(t)
	})
}

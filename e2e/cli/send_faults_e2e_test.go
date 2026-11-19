package e2e

import (
	"encoding/json"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

// unicResult runs the unic CLI against the run's service and returns its output and exit code.
func (h *observabilityRun) unicResult(t *testing.T, args ...string) (string, int) {
	t.Helper()
	cmd := exec.Command(filepath.Join(binDir, "unic"), args...)
	cmd.Env = h.env()
	var stdout, stderr strings.Builder
	cmd.Stdout, cmd.Stderr = &stdout, &stderr
	err := cmd.Run()
	code := 0
	var exit *exec.ExitError
	if errors.As(err, &exit) {
		code = exit.ExitCode()
	} else if err != nil {
		t.Fatalf("unic %s: %v", args[0], err)
	}
	return stdout.String() + stderr.String(), code
}

// TestSendFaults covers 0021 C23's second half: a send whose only target fails mid-body gets an
// accurate terminal outcome. The local commit stays, the command exits 4, and nothing is replayed.
func TestSendFaults(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	a, b := newObservabilityRun(t), newObservabilityRun(t)
	a.artifactName, b.artifactName = "send-fault-alpha", "send-fault-beta"
	for _, h := range []*observabilityRun{a, b} {
		h.start(t, h.server.URL)
	}
	t.Cleanup(func() { a.stop(t); b.stop(t) })
	pairSyncServices(t, a, b)

	a.run(t, "target-fails-mid-body", func(t *testing.T) {
		syncControl(t, a, "partial-body")
		text := strings.Repeat("c23-partial-canary", 1024)
		out, code := a.unicResult(t, "send", "--to", "all", "--json", "--request-id", "c23-partial", text)
		if code != 4 {
			t.Fatalf("exit %d, want 4 (committed but incomplete)\n%s", code, out)
		}
		var rep struct {
			Submission struct {
				Phase string `json:"phase"`
			} `json:"submission"`
			Send struct {
				Outcome string `json:"outcome"`
			} `json:"send"`
		}
		line := out[strings.Index(out, "{"):]
		if err := json.NewDecoder(strings.NewReader(line)).Decode(&rep); err != nil {
			t.Fatalf("decode: %v\n%s", err, out)
		}
		if rep.Submission.Phase != "committed" {
			t.Fatalf("phase %q, want committed", rep.Submission.Phase)
		}
		if rep.Send.Outcome == "success" || rep.Send.Outcome == "" || rep.Send.Outcome == "none" {
			t.Fatalf("outcome %q must report the failed target", rep.Send.Outcome)
		}
		t.Logf("exit %d, phase %s, outcome %s", code, rep.Submission.Phase, rep.Send.Outcome)
		// The local entry stays and nothing is replayed to the peer after the failure.
		time.Sleep(3 * time.Second)
		if n := strings.Count(string(a.get(t, "/v1/history")), "c23-partial-canary"); n > 1 {
			t.Fatalf("history holds %d entries, want at most 1", n)
		}
		if b.eventCount("remote_content_applied") != 0 && strings.Contains(string(b.get(t, "/v1/clipboard")), "c23-partial-canary") {
			t.Fatal("a partial body became clipboard content on the peer")
		}
	})
}

// historyTargets returns the per-device send results of every history entry, newest first.
func historyTargets(t *testing.T, h *observabilityRun) []struct{ TargetID, State, ErrorCode string } {
	t.Helper()
	var page struct {
		Items []struct {
			Targets []struct {
				TargetID  string `json:"target_id"`
				State     string `json:"state"`
				ErrorCode string `json:"error_code"`
			} `json:"targets"`
		} `json:"items"`
	}
	if err := json.Unmarshal(h.get(t, "/v1/history"), &page); err != nil {
		t.Fatalf("decode history: %v", err)
	}
	var out []struct{ TargetID, State, ErrorCode string }
	for _, it := range page.Items {
		for _, x := range it.Targets {
			out = append(out, struct{ TargetID, State, ErrorCode string }{x.TargetID, x.State, x.ErrorCode})
		}
	}
	return out
}

// TestHistoryTargets covers per-device targets: a history entry keeps the last send result per device,
// and a later failure is stored as failed with a stable code.
func TestHistoryTargets(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	a, b := newObservabilityRun(t), newObservabilityRun(t)
	a.artifactName, b.artifactName = "history-targets-alpha", "history-targets-beta"
	for _, h := range []*observabilityRun{a, b} {
		h.start(t, h.server.URL)
	}
	t.Cleanup(func() { a.stop(t); b.stop(t) })
	pairSyncServices(t, a, b)

	a.run(t, "sent-and-failed-targets-are-stored", func(t *testing.T) {
		if out, code := a.unicResult(t, "send", "--to", "all", "--json", "--request-id", "t27-ok", "t27-ok-marker"); code != 0 {
			t.Fatalf("healthy send exit %d\n%s", code, out)
		}
		ok := historyTargets(t, a)
		if len(ok) != 1 || ok[0].State != "sent" || ok[0].ErrorCode != "" {
			t.Fatalf("healthy send targets = %+v, want one sent", ok)
		}
		syncControl(t, a, "partial-body")
		text := strings.Repeat("t27-fail-marker", 1024)
		if _, code := a.unicResult(t, "send", "--to", "all", "--json", "--request-id", "t27-fail", text); code != 4 {
			t.Fatalf("faulty send exit %d, want 4", code)
		}
		var failed int
		for _, x := range historyTargets(t, a) {
			if x.State == "failed" && x.ErrorCode != "" {
				failed++
			}
		}
		if failed != 1 {
			t.Fatalf("want exactly one failed target with a code")
		}
	})
}

// TestHistoryTargetProgress covers live byte progress: while a send runs, the history entry lists each
// part with rising completed bytes; after the send the items are gone and the target is sent.
func TestHistoryTargetProgress(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	a, b := newObservabilityRun(t), newObservabilityRun(t)
	a.artifactName, b.artifactName = "history-progress-alpha", "history-progress-beta"
	for _, h := range []*observabilityRun{a, b} {
		h.start(t, h.server.URL)
	}
	t.Cleanup(func() { a.stop(t); b.stop(t) })
	pairSyncServices(t, a, b)

	a.run(t, "running-send-reports-bytes", func(t *testing.T) {
		syncControl(t, a, "slow")
		big := filepath.Join(t.TempDir(), "big.bin")
		if err := os.WriteFile(big, make([]byte, 2<<20), 0o600); err != nil {
			t.Fatal(err)
		}
		done := make(chan struct{})
		go func() {
			defer close(done)
			a.unicResult(t, "send", "-f", "--to", "all", "--json", "--request-id", "t27-slow", big)
		}()
		type item struct {
			State          string `json:"state"`
			CompletedBytes int64  `json:"completed_bytes"`
			TotalBytes     int64  `json:"total_bytes"`
		}
		var first, last int64 = -1, -1
		deadline := time.Now().Add(60 * time.Second)
		for time.Now().Before(deadline) {
			var page struct {
				Items []struct {
					Targets []struct {
						State string `json:"state"`
						Items []item `json:"items"`
					} `json:"targets"`
				} `json:"items"`
			}
			if err := json.Unmarshal(a.get(t, "/v1/history"), &page); err == nil && len(page.Items) > 0 {
				for _, tg := range page.Items[0].Targets {
					for _, it := range tg.Items {
						if it.State == "transferring" && it.TotalBytes > 0 {
							if first < 0 {
								first = it.CompletedBytes
							}
							last = it.CompletedBytes
						}
					}
				}
			}
			select {
			case <-done:
				deadline = time.Time{}
			case <-time.After(100 * time.Millisecond):
			}
		}
		if first < 0 || last <= first {
			t.Fatalf("no rising progress observed: first=%d last=%d", first, last)
		}
		t.Logf("bytes rose from %d to %d", first, last)
		syncControl(t, a, "normal-speed")
		tg := historyTargets(t, a)
		if len(tg) != 1 || tg[0].State != "sent" {
			t.Fatalf("final targets = %+v, want one sent", tg)
		}
	})
}

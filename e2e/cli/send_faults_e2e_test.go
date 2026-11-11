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

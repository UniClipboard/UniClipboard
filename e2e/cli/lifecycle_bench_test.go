package e2e

// Cost and residue comparison for 0021: HEAD binaries (no automatic service) against the candidate.
// Both sides are plain release builds without the race detector or fixture tags.
// The thresholds below were fixed before the measurement run and are not changed after seeing results.
// They were fixed after the implementation existed, so this is not a pre-implementation baseline.

import (
	"context"
	"encoding/json"
	"net"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"
	"sync"
	"syscall"
	"testing"
	"time"
)

const (
	warmOverheadMax   = 50 * time.Millisecond // candidate warm status median minus baseline median
	coldStatusMax     = 5 * time.Second       // candidate cold status median
	concurrentColdMax = 15 * time.Second      // 16 simultaneous cold commands, wall time
	residueLogMax     = 64 << 10              // cli-daemon.log after repeated cold starts
	repeatCold        = 30
)

func median(d []time.Duration) time.Duration {
	sort.Slice(d, func(i, j int) bool { return d[i] < d[j] })
	return d[len(d)/2]
}

func TestServiceLifecycleResources(t *testing.T) {
	baseDir, candDir := os.Getenv("UNIC_BENCH_BASE"), os.Getenv("UNIC_BENCH_CAND")
	if baseDir == "" || candDir == "" {
		t.Skip("run through `make service-lifecycle-bench`")
	}
	root := shortTemp(t)
	env := func(name string) (home, sock string, env []string) {
		home = filepath.Join(root, name)
		_ = os.MkdirAll(home, 0o700)
		_ = os.WriteFile(filepath.Join(home, "config.toml"), []byte("\n[telemetry]\nenabled=false\n"), 0o600)
		sock = filepath.Join(root, name+".sock")
		return home, sock, append(os.Environ(), "UNIC_HOME="+home, "UNIC_SOCK="+sock, "UNIC_RENDEZVOUS_URL=http://127.0.0.1:1")
	}
	timeStatus := func(bin string, e []string) time.Duration {
		cmd := exec.Command(filepath.Join(bin, "unic"), "status")
		cmd.Env = e
		began := time.Now()
		if out, err := cmd.CombinedOutput(); err != nil {
			t.Fatalf("status: %v %s", err, out)
		}
		return time.Since(began)
	}
	warm := func(bin, name string) time.Duration {
		_, sock, e := env(name)
		d := exec.Command(filepath.Join(bin, "unicd"))
		d.Env = e
		if err := d.Start(); err != nil {
			t.Fatal(err)
		}
		defer func() { _ = d.Process.Signal(syscall.SIGTERM); _ = d.Wait() }()
		// The socket file appears several seconds before the server accepts requests, so wait for a real answer.
		hc := &http.Client{Timeout: 20 * time.Second, Transport: &http.Transport{DialContext: func(ctx context.Context, _, _ string) (net.Conn, error) {
			return (&net.Dialer{}).DialContext(ctx, "unix", sock)
		}}}
		for range 100 {
			if resp, err := hc.Get("http://unic/v1/service"); err == nil {
				_ = resp.Body.Close()
				break
			}
			time.Sleep(50 * time.Millisecond)
		}
		var samples []time.Duration
		for range 21 {
			samples = append(samples, timeStatus(bin, e))
		}
		return median(samples[1:]) // the first call warms the process cache
	}
	base := warm(baseDir, "warm-base")
	cand := warm(candDir, "warm-cand")
	if cand-base > warmOverheadMax {
		t.Errorf("warm status overhead %s exceeds %s (baseline %s, candidate %s)", cand-base, warmOverheadMax, base, cand)
	}

	// Cold commands start and stop a temporary service each time.
	home, sock, e := env("cold")
	var cold []time.Duration
	for range 9 {
		cold = append(cold, timeStatus(candDir, e))
	}
	if m := median(cold); m > coldStatusMax {
		t.Errorf("cold status median %s exceeds %s", m, coldStatusMax)
	}

	// Many simultaneous cold commands share one generation.
	_, _, ce := env("burst")
	var wg sync.WaitGroup
	maxProcs := 0
	stop, sampled := make(chan struct{}), make(chan struct{})
	go func() {
		defer close(sampled)
		for {
			select {
			case <-stop:
				return
			default:
				out, _ := exec.Command("pgrep", "-f", filepath.Join(candDir, "unicd")).Output()
				maxProcs = max(maxProcs, len(strings.Fields(string(out))))
				time.Sleep(20 * time.Millisecond)
			}
		}
	}()
	began := time.Now()
	for range 16 {
		wg.Add(1)
		go func() {
			defer wg.Done()
			cmd := exec.Command(filepath.Join(candDir, "unic"), "status")
			cmd.Env = ce
			if out, err := cmd.CombinedOutput(); err != nil {
				t.Errorf("burst status: %v %s", err, out)
			}
		}()
	}
	wg.Wait()
	burst := time.Since(began)
	close(stop)
	<-sampled
	if burst > concurrentColdMax {
		t.Errorf("16 concurrent cold commands took %s, limit %s", burst, concurrentColdMax)
	}
	if maxProcs > 1 {
		t.Errorf("observed %d unicd processes at once; expected one shared generation", maxProcs)
	}

	// Repeated invocations leave nothing behind and nothing grows.
	for range repeatCold {
		timeStatus(candDir, e)
	}
	if out, _ := exec.Command("pgrep", "-f", filepath.Join(candDir, "unicd")).Output(); len(strings.Fields(string(out))) != 0 {
		t.Errorf("unicd processes remain after repeated cold commands: %s", out)
	}
	if _, err := os.Stat(sock); err == nil {
		t.Error("socket remains after repeated cold commands")
	}
	if info, err := os.Stat(filepath.Join(home, "cli-daemon.log")); err == nil && info.Size() > residueLogMax {
		t.Errorf("cli-daemon.log grew to %d bytes", info.Size())
	}
	entries, _ := os.ReadDir(home)
	summary := map[string]any{
		"thresholds_ms": map[string]int64{"warm_overhead_max": warmOverheadMax.Milliseconds(), "cold_status_max": coldStatusMax.Milliseconds(), "concurrent_cold_max": concurrentColdMax.Milliseconds()},
		"baseline":      map[string]any{"commit": "clean HEAD checkout", "warm_status_median_ms": base.Milliseconds(), "cold_status": "not available: the command failed without a running service"},
		"candidate": map[string]any{
			"warm_status_median_ms": cand.Milliseconds(), "cold_status_median_ms": median(cold).Milliseconds(),
			"concurrent_16_wall_ms": burst.Milliseconds(), "max_simultaneous_unicd": maxProcs,
			"state_dir_entries_after_repeats": len(entries), "repeat_cold_invocations": repeatCold,
		},
		"build": "go build, no race detector, no fixture tags, same toolchain on both sides",
	}
	b, _ := json.MarshalIndent(summary, "", "  ")
	dir := os.Getenv("UNIC_E2E_ARTIFACTS")
	if dir == "" {
		dir = t.TempDir()
	}
	_ = os.MkdirAll(dir, 0o700)
	_ = os.WriteFile(filepath.Join(dir, "resource-bench.json"), append(b, '\n'), 0o600)
	t.Logf("%s", b)
}

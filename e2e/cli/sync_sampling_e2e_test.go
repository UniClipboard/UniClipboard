package e2e

import (
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"runtime"
	"testing"
	"time"
)

func TestSyncObservabilitySampling(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	for _, peers := range []int{0, 1, 8} {
		t.Run(fmt.Sprint(peers), func(t *testing.T) {
			h := newObservabilityRun(t)
			h.artifactName = fmt.Sprintf("sync-sampling-%d", peers)
			h.production = true // Keep the 60-second exporter and 10-second sync sampler.
			h.syncProbe = true
			h.start(t, h.server.URL)
			for i := 0; i < peers; i++ {
				peer := newObservabilityRun(t)
				peer.artifactName = fmt.Sprintf("sync-sampling-%d-peer-%d", peers, i)
				peer.start(t, "")
				pairSyncServices(t, h, peer)
			}
			for _, enabled := range []bool{false, true} {
				h.run(t, fmt.Sprintf("metrics-%t", enabled), func(t *testing.T) {
					h.cli(t, "config", "set", "telemetry.enabled", fmt.Sprint(enabled))
					samples := func() []int64 {
						var values []int64
						if err := json.Unmarshal(h.get(t, "/test/sync-collection"), &values); err != nil {
							t.Fatal("decode sync collection samples")
						}
						return values
					}
					if enabled {
						h.eventually(t, func() bool { return len(samples()) != 0 })
						// Allow the config watcher and synchronous set to settle.
						time.Sleep(time.Second)
					}
					before := len(samples())
					pid := h.cmd.Process.Pid
					start := processWakeCounters(t, pid)
					cpu := processCPU(t, pid)
					started := time.Now()
					time.Sleep(21 * time.Second)
					elapsed := time.Since(started)
					cpu = processCPU(t, pid) - cpu
					end := processWakeCounters(t, pid)
					durations := samples()[before:]
					wakes := map[string]uint64{}
					for key, count := range end {
						if count < start[key] {
							t.Fatal("process wake counter regressed")
						}
						wakes[key] = count - start[key]
					}
					var runtimeState map[string]uint64
					if err := json.Unmarshal(h.get(t, "/test/resources"), &runtimeState); err != nil {
						t.Fatal("decode resource counters")
					}
					h.resourceSamples = append(h.resourceSamples, map[string]any{
						"metrics_enabled": enabled, "sessions": peers, "window_ns": elapsed.Nanoseconds(),
						"collector_wall_ns": durations, "process_wakeups": wakes,
						"process_wakeups_available": len(wakes) > 0, "cpu_seconds": cpu,
						"rss_kib": processRSS(t, pid), "runtime": runtimeState,
					})
					want := 0
					if enabled && peers != 0 {
						want = 2
					}
					h.assertions = append(h.assertions, map[string]any{
						"scenario": "collection cadence", "metrics_enabled": enabled, "sessions": peers,
						"expected_calls": want, "actual_calls": len(durations),
					})
					if len(durations) != want {
						t.Errorf("collector ran %d times in %s, want %d", len(durations), elapsed, want)
					}
				})
			}
		})
	}
}

// proc_pid_rusage reports process-wide Darwin counters. These measurements
// include all process activity during the window, including tailcat and Go.
func processWakeCounters(t *testing.T, pid int) map[string]uint64 {
	t.Helper()
	if runtime.GOOS != "darwin" {
		return nil
	}
	script := `
import ctypes as c
import json
import sys

class RUsageV0(c.Structure):
    _fields_ = [("uuid", c.c_ubyte * 16)] + [
        (name, c.c_uint64) for name in (
            "user_ns", "system_ns", "package_idle", "interrupt", "pageins",
            "wired", "resident", "footprint", "proc_start", "proc_exit",
        )
    ]

usage = RUsageV0()
proc_pid_rusage = c.CDLL("libproc.dylib", use_errno=True).proc_pid_rusage
proc_pid_rusage.argtypes = (c.c_int, c.c_int, c.POINTER(RUsageV0))
if proc_pid_rusage(int(sys.argv[1]), 0, c.byref(usage)) != 0:
    sys.exit(c.get_errno() or 1)
print(json.dumps({"package_idle": usage.package_idle, "interrupt": usage.interrupt}))
`
	out, err := exec.Command("/usr/bin/python3", "-c", script, fmt.Sprint(pid)).Output()
	if err != nil {
		t.Fatalf("read Darwin process wakeups: %v", err)
	}
	var counters map[string]uint64
	if err := json.Unmarshal(out, &counters); err != nil {
		t.Fatal("decode Darwin process wakeups")
	}
	return counters
}

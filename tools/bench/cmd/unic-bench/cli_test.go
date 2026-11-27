package main

import (
	"bytes"
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"

	"github.com/uniclipboard/unic/tools/bench/internal/fixture"
	"github.com/uniclipboard/unic/tools/bench/internal/report"
	"github.com/uniclipboard/unic/tools/bench/internal/sampler"
)

func TestCLISeedCorpusOnly(t *testing.T) {
	outDir := filepath.Join(t.TempDir(), "corpus")
	var stdout, stderr bytes.Buffer
	code := runCLI(context.Background(), []string{
		"seed", "--preset", "empty", "--out", outDir, "--json",
	}, &stdout, &stderr)
	if code != exitOK {
		t.Fatalf("seed exit=%d stderr=%s", code, stderr.String())
	}

	var out seedOutput
	if err := json.Unmarshal(stdout.Bytes(), &out); err != nil {
		t.Fatalf("unmarshal seed json: %v\nraw=%s", err, stdout.String())
	}
	if out.Manifest == nil || out.Manifest.Preset != fixture.PresetEmpty {
		t.Fatalf("unexpected seed manifest: %+v", out.Manifest)
	}
	if _, err := os.Stat(out.ManifestPath); err != nil {
		t.Fatalf("manifest missing: %v", err)
	}
}

func TestCLIComparePassAndFailOnRegression(t *testing.T) {
	dir := t.TempDir()
	basePath := filepath.Join(dir, "base.json")
	goodPath := filepath.Join(dir, "good.json")
	badPath := filepath.Join(dir, "bad.json")
	budgetPath := filepath.Join(dir, "budget.json")

	mk := func(id string, mem uint64, cpu, p95, slope float64) *report.Report {
		return &report.Report{
			SchemaVersion:  report.SchemaVersion,
			RunID:          id,
			GeneratedAtUTC: "2026-11-27T00:00:00Z",
			Track:          "production",
			Preset:         "standard",
			Environment:    report.EnvironmentInfo{OS: "darwin", HostArch: "arm64", LogicalCPUs: 8},
			Scenarios: []report.ScenarioResult{
				{
					ID:     "bg-idle-normal",
					Status: "PASS",
					Phases: []sampler.PhaseSummary{
						{Phase: "steady", SteadyMedianBytes: mem, CPUPercentMean: cpu},
					},
					Latencies: map[string]report.LatencyMetric{
						"ipc_to_visible_ms": {Name: "ipc_to_visible_ms", Count: 10, P95Ms: p95},
					},
					LeakCheck: &report.LeakSummary{
						Metric:             "tray_mem",
						Rounds:             10,
						SlopeBytesPerRound: slope,
						Passed:             slope <= (1 << 20),
					},
				},
			},
		}
	}

	if err := report.WriteJSON(basePath, mk("base", 100<<20, 1.0, 40.0, 50<<10)); err != nil {
		t.Fatal(err)
	}
	if err := report.WriteJSON(goodPath, mk("good", 104<<20, 1.1, 60.0, 80<<10)); err != nil {
		t.Fatal(err)
	}
	if err := report.WriteJSON(badPath, mk("bad", 130<<20, 2.0, 200.0, 3<<20)); err != nil {
		t.Fatal(err)
	}
	budgetRaw := `{"schema_version":1,"scenarios":{"bg-idle-normal":{"max_primary_memory_bytes":125829120,"max_leak_slope_bytes_per_round":1048576}}}`
	if err := os.WriteFile(budgetPath, []byte(budgetRaw), 0o600); err != nil {
		t.Fatal(err)
	}

	var stdout, stderr bytes.Buffer
	code := runCLI(context.Background(), []string{
		"compare", basePath, goodPath, "--budget", budgetPath, "--fail-on-regression",
	}, &stdout, &stderr)
	if code != exitOK {
		t.Fatalf("compare good exit=%d stdout=%s stderr=%s", code, stdout.String(), stderr.String())
	}

	stdout.Reset()
	stderr.Reset()
	code = runCLI(context.Background(), []string{
		"compare", basePath, badPath, "--budget", budgetPath, "--fail-on-regression",
	}, &stdout, &stderr)
	if code != exitRegression {
		t.Fatalf("compare bad exit=%d, want %d; stdout=%s stderr=%s", code, exitRegression, stdout.String(), stderr.String())
	}
	if !strings.Contains(stdout.String(), "ALERT") || !strings.Contains(stdout.String(), "BUDGET_EXCEEDED") {
		t.Fatalf("expected ALERT and BUDGET_EXCEEDED in output:\n%s", stdout.String())
	}
}

func TestCLIDoctorJSON(t *testing.T) {
	if runtime.GOOS != "darwin" && runtime.GOOS != "windows" {
		t.Skip("doctor self-sampling check applies on darwin/windows")
	}
	var stdout, stderr bytes.Buffer
	code := runCLI(context.Background(), []string{"doctor", "--json"}, &stdout, &stderr)
	if code != exitOK {
		t.Fatalf("doctor exit=%d stdout=%s stderr=%s", code, stdout.String(), stderr.String())
	}
	var rep doctorReport
	if err := json.Unmarshal(stdout.Bytes(), &rep); err != nil {
		t.Fatalf("unmarshal doctor json: %v\nraw=%s", err, stdout.String())
	}
	if rep.SelfSample.PrimaryMemoryBytes == 0 {
		t.Fatalf("expected non-zero self primary memory: %+v", rep.SelfSample)
	}
}

func TestCLIRunHelpAndBadFlags(t *testing.T) {
	var stdout, stderr bytes.Buffer
	if code := runCLI(context.Background(), []string{"run", "--help"}, &stdout, &stderr); code != exitOK {
		t.Fatalf("run --help exit=%d stderr=%s", code, stderr.String())
	}
	if !strings.Contains(stderr.String(), "-suite") || !strings.Contains(stderr.String(), "-track") {
		t.Fatalf("expected run flags in help: %s", stderr.String())
	}

	stdout.Reset()
	stderr.Reset()
	if code := runCLI(context.Background(), []string{"run", "--track", "bogus"}, &stdout, &stderr); code != exitUsage {
		t.Fatalf("run invalid track exit=%d, want %d", code, exitUsage)
	}
}

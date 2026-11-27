package report

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/uniclipboard/unic/tools/bench/internal/sampler"
)

func sampleReport(runID string, memMedian uint64, cpuPct float64, p95Ms float64, leakSlope float64) *Report {
	return &Report{
		SchemaVersion:  SchemaVersion,
		RunID:          runID,
		GeneratedAtUTC: "2026-11-27T00:00:00Z",
		Track:          "production",
		Preset:         "standard",
		Environment: EnvironmentInfo{
			OS:                  "darwin",
			HostArch:            "arm64",
			OSVersion:           "27.0",
			CPUModel:            "Apple Silicon",
			LogicalCPUs:         8,
			PhysicalMemoryBytes: 16 << 30,
		},
		Fixture: FixtureInfo{
			Preset:     "standard",
			Seed:       20261120,
			TextCount:  200,
			ImageCount: 12,
			FileCount:  5,
		},
		ClipboardIntegrity: ClipboardCheck{
			Available:      true,
			BeforeSequence: 42,
			AfterSequence:  42,
			Unchanged:      true,
		},
		Scenarios: []ScenarioResult{
			{
				ID:            "bg-idle-normal",
				Track:         "production",
				Dataset:       "standard",
				Preset:        "standard",
				Rounds:        3,
				Status:        "PASS",
				ExpectedRoles: []string{"unicd", "tray", "quick-panel"},
				Phases: []sampler.PhaseSummary{
					{
						Phase:                  "steady",
						SteadyMedianBytes:      memMedian,
						SteadyP95Bytes:         memMedian + (5 << 20),
						PeakPrimaryMemoryBytes: memMedian + (15 << 20),
						EarlyPeakBytes:         memMedian + (15 << 20),
						CPUPercentMean:         cpuPct,
						IdleWakeupsPerSec:      1.2,
					},
				},
			},
			{
				ID:      "qp-lifecycle",
				Track:   "interactive",
				Dataset: "standard",
				Preset:  "standard",
				Rounds:  10,
				Status:  "PASS",
				Latencies: map[string]LatencyMetric{
					"ipc_to_visible_ms": {
						Name:  "ipc_to_visible_ms",
						Count: 10,
						P50Ms: p95Ms * 0.7,
						P95Ms: p95Ms,
						P99Ms: p95Ms * 1.1,
						MaxMs: p95Ms * 1.2,
					},
				},
				LeakCheck: &LeakSummary{
					Metric:             "tray_plus_panel_primary_memory",
					Rounds:             10,
					SlopeBytesPerRound: leakSlope,
					Passed:             leakSlope <= (1 << 20),
				},
			},
		},
	}
}

func TestWriteReadJSONMarkdownAndSHA256SUMS(t *testing.T) {
	dir := t.TempDir()
	r := sampleReport("run-01", 80<<20, 1.5, 45.0, 64<<10)

	jsonPath := filepath.Join(dir, "report.json")
	if err := WriteJSON(jsonPath, r); err != nil {
		t.Fatalf("WriteJSON: %v", err)
	}
	loaded, err := ReadJSON(jsonPath)
	if err != nil {
		t.Fatalf("ReadJSON: %v", err)
	}
	if loaded.RunID != "run-01" || len(loaded.Scenarios) != 2 {
		t.Fatalf("unexpected loaded report: %+v", loaded)
	}

	md := RenderMarkdown(loaded)
	for _, want := range []string{"# Desktop Resource Benchmark Report", "bg-idle-normal", "qp-lifecycle", "ipc_to_visible_ms"} {
		if !strings.Contains(md, want) {
			t.Fatalf("markdown missing %q", want)
		}
	}
	mdPath := filepath.Join(dir, "report.md")
	if err := os.WriteFile(mdPath, []byte(md), 0o600); err != nil {
		t.Fatal(err)
	}

	sumsPath, err := WriteSHA256SUMS(dir, "report.json", "report.md")
	if err != nil {
		t.Fatalf("WriteSHA256SUMS: %v", err)
	}
	sumsRaw, err := os.ReadFile(sumsPath)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(sumsRaw), "  report.json\n") || !strings.Contains(string(sumsRaw), "  report.md\n") {
		t.Fatalf("unexpected SHA256SUMS:\n%s", string(sumsRaw))
	}
}

func TestComparePassAndRegressions(t *testing.T) {
	base := sampleReport("base", 100<<20, 2.0, 50.0, 100<<10)

	// 1. Clean candidate within all thresholds and budget.
	good := sampleReport("good", 105<<20, 2.2, 80.0, 256<<10)
	budget := &BudgetFile{
		Scenarios: map[string]ScenarioBudget{
			"bg-idle-normal": {
				MaxPrimaryMemoryBytes: 120 << 20,
				MaxCPUPercent:         5.0,
			},
			"qp-lifecycle": {
				MaxLatencyP95Ms:           map[string]float64{"ipc_to_visible_ms": 150.0},
				MaxLeakSlopeBytesPerRound: 1 << 20,
			},
		},
	}
	resGood, err := Compare(base, good, CompareOptions{Thresholds: DefaultThresholds(), Budget: budget})
	if err != nil {
		t.Fatalf("Compare good: %v", err)
	}
	if resGood.HasRegressions() {
		t.Fatalf("expected no regressions for good candidate, got %+v", resGood.Findings)
	}

	// 2. Regression candidate tripping memory (>+10%), CPU (>+20%), latency (>+100ms), and leak (>1 MiB/round).
	bad := sampleReport("bad", 125<<20, 3.0, 180.0, 2<<20)
	resBad, err := Compare(base, bad, CompareOptions{Thresholds: DefaultThresholds(), Budget: budget})
	if err != nil {
		t.Fatalf("Compare bad: %v", err)
	}
	if !resBad.HasRegressions() {
		t.Fatal("expected regressions for bad candidate")
	}

	seen := map[string]bool{}
	for _, f := range resBad.Findings {
		seen[string(f.Severity)+":"+f.Metric] = true
	}
	for _, want := range []string{
		"ALERT:steady_median_memory",
		"ALERT:cpu_percent_mean",
		"ALERT:ipc_to_visible_ms.p95_ms",
		"ALERT:tray_plus_panel_primary_memory",
		"BUDGET_EXCEEDED:steady_median_memory",
		"BUDGET_EXCEEDED:ipc_to_visible_ms.p95_ms",
		"BUDGET_EXCEEDED:tray_plus_panel_primary_memory",
	} {
		if !seen[want] {
			t.Fatalf("missing finding %q in %+v", want, resBad.Findings)
		}
	}

	md := RenderCompareMarkdown(resBad)
	if !strings.Contains(md, "REGRESSION / BUDGET ALERT") {
		t.Fatalf("expected alert banner in compare markdown:\n%s", md)
	}
	for _, col := range []string{"| Scenario | Track | Dataset | Phase |", "| Scenario | Track | Dataset | Metric |", "| `production` | `standard` |", "| `interactive` | `standard` |"} {
		if !strings.Contains(md, col) {
			t.Fatalf("expected compare markdown to contain %q:\n%s", col, md)
		}
	}
	if len(resGood.PhaseDiffs) == 0 || resGood.PhaseDiffs[0].Track != "production" || resGood.PhaseDiffs[0].Dataset != "standard" {
		t.Fatalf("expected Track/Dataset in PhaseDiffs, got %+v", resGood.PhaseDiffs)
	}
	if len(resGood.LatencyDiffs) == 0 || resGood.LatencyDiffs[0].Track != "interactive" || resGood.LatencyDiffs[0].Dataset != "standard" {
		t.Fatalf("expected Track/Dataset in LatencyDiffs, got %+v", resGood.LatencyDiffs)
	}
}

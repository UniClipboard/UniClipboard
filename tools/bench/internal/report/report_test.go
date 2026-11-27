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

func TestCompareIncompatibleAndMissingBaseline(t *testing.T) {
	mac := sampleReport("mac-base", 80<<20, 1.5, 45.0, 0)
	win := sampleReport("win-cand", 120<<20, 2.5, 90.0, 0)
	win.Environment.OS = "windows"
	budget := &BudgetFile{
		Scenarios: map[string]ScenarioBudget{
			"bg-idle-normal": {MaxPrimaryMemoryBytes: 100 << 20},
		},
	}

	// 1. Cross-OS comparison must suppress relative deltas and alerts while keeping budget checks.
	cross, err := Compare(mac, win, CompareOptions{Thresholds: DefaultThresholds(), Budget: budget})
	if err != nil {
		t.Fatalf("Compare cross-OS: %v", err)
	}
	if !cross.Incompatible || !strings.Contains(cross.IncompatibilityReason, "OS differs") {
		t.Fatalf("expected Incompatible with OS reason, got %+v", cross)
	}
	for _, pd := range cross.PhaseDiffs {
		if !pd.Incompatible || pd.MemDeltaPct != 0 || pd.CPUDeltaPct != 0 {
			t.Fatalf("expected zeroed incompatible phase diff, got %+v", pd)
		}
	}
	for _, ld := range cross.LatencyDiffs {
		if !ld.Incompatible || ld.DeltaMs != 0 {
			t.Fatalf("expected zeroed incompatible latency diff, got %+v", ld)
		}
	}
	hasBudget := false
	for _, f := range cross.Findings {
		if f.Severity == SeverityAlert && (f.Metric == "steady_median_memory" || f.Metric == "cpu_percent_mean") {
			t.Fatalf("cross-OS comparison must not emit relative ALERT: %+v", f)
		}
		if f.Severity == SeverityBudgetExceeded {
			hasBudget = true
		}
	}
	if !hasBudget {
		t.Fatal("expected absolute BUDGET_EXCEEDED finding to still be evaluated")
	}

	// 2. Cross-track comparison must also be marked incompatible.
	prod := sampleReport("prod", 80<<20, 1.0, 40.0, 0)
	inter := sampleReport("inter", 160<<20, 2.0, 80.0, 0)
	inter.Track = "interactive"
	inter.Scenarios[0].Track = "interactive"
	trackRes, err := Compare(prod, inter, CompareOptions{Thresholds: DefaultThresholds()})
	if err != nil {
		t.Fatalf("Compare cross-track: %v", err)
	}
	if !trackRes.Incompatible || !strings.Contains(trackRes.IncompatibilityReason, "track differs") {
		t.Fatalf("expected track mismatch incompatibility, got %+v", trackRes)
	}
	for _, pd := range trackRes.PhaseDiffs {
		if !pd.Incompatible || pd.MemDeltaPct != 0 {
			t.Fatalf("expected zeroed cross-track delta, got %+v", pd)
		}
	}

	// 3. Missing baseline must mark BaseMissing=true and render "-" in Markdown.
	emptyBase := &Report{
		SchemaVersion: SchemaVersion,
		RunID:         "empty-base",
		Track:         "production",
		Environment:   EnvironmentInfo{OS: "darwin", HostArch: "arm64"},
	}
	candOnly, err := Compare(emptyBase, prod, CompareOptions{Thresholds: DefaultThresholds()})
	if err != nil {
		t.Fatalf("Compare cand-only: %v", err)
	}
	if len(candOnly.PhaseDiffs) == 0 || !candOnly.PhaseDiffs[0].BaseMissing {
		t.Fatalf("expected BaseMissing=true on PhaseDiff, got %+v", candOnly.PhaseDiffs)
	}
	if len(candOnly.LatencyDiffs) == 0 || !candOnly.LatencyDiffs[0].BaseMissing {
		t.Fatalf("expected BaseMissing=true on LatencyDiff, got %+v", candOnly.LatencyDiffs)
	}
	md := RenderCompareMarkdown(candOnly)
	if strings.Contains(md, "| 0 B |") || strings.Contains(md, "| +0.0% |") {
		t.Fatalf("missing baseline leaked 0 B / +0.0%% into markdown:\n%s", md)
	}

	// 4. Missing candidate (both whole-scenario missing and partial phase/latency missing)
	// must emit CandMissing=true and render "-" without false deltas.
	partialCand := &Report{
		SchemaVersion: SchemaVersion,
		RunID:         "partial-cand",
		Track:         "production",
		Environment:   EnvironmentInfo{OS: "darwin", HostArch: "arm64"},
		Scenarios: []ScenarioResult{
			{ID: "bg-idle-normal", Track: "production", Dataset: "standard", Status: "PASS"},
		},
	}
	missCandWhole, err := Compare(prod, emptyBase, CompareOptions{Thresholds: DefaultThresholds()})
	if err != nil {
		t.Fatalf("Compare missCandWhole: %v", err)
	}
	if len(missCandWhole.PhaseDiffs) == 0 || !missCandWhole.PhaseDiffs[0].CandMissing {
		t.Fatalf("expected CandMissing=true on whole-scenario missing PhaseDiff, got %+v", missCandWhole.PhaseDiffs)
	}
	if len(missCandWhole.LatencyDiffs) == 0 || !missCandWhole.LatencyDiffs[0].CandMissing {
		t.Fatalf("expected CandMissing=true on whole-scenario missing LatencyDiff, got %+v", missCandWhole.LatencyDiffs)
	}
	missCandPart, err := Compare(prod, partialCand, CompareOptions{Thresholds: DefaultThresholds()})
	if err != nil {
		t.Fatalf("Compare missCandPart: %v", err)
	}
	if len(missCandPart.PhaseDiffs) == 0 || !missCandPart.PhaseDiffs[0].CandMissing {
		t.Fatalf("expected CandMissing=true on partial PhaseDiff, got %+v", missCandPart.PhaseDiffs)
	}
	if len(missCandPart.LatencyDiffs) == 0 || !missCandPart.LatencyDiffs[0].CandMissing {
		t.Fatalf("expected CandMissing=true on partial LatencyDiff, got %+v", missCandPart.LatencyDiffs)
	}
	missMd := RenderCompareMarkdown(missCandPart)
	if !strings.Contains(missMd, "| - | - |") {
		t.Fatalf("expected missing candidate dashes in markdown:\n%s", missMd)
	}

	// 5. Same-OS, same-track cross-dataset (empty vs standard) must NOT fall back to ID pairing.
	emptyDS := sampleReport("empty-ds", 80<<20, 1.0, 40.0, 0)
	emptyDS.Scenarios = emptyDS.Scenarios[:1]
	emptyDS.Scenarios[0].Dataset = "empty"
	stdDS := sampleReport("std-ds", 160<<20, 1.0, 40.0, 0)
	stdDS.Scenarios = stdDS.Scenarios[:1]
	stdDS.Scenarios[0].Dataset = "standard"
	crossDS, err := Compare(emptyDS, stdDS, CompareOptions{Thresholds: DefaultThresholds()})
	if err != nil {
		t.Fatalf("Compare crossDS: %v", err)
	}
	if len(crossDS.PhaseDiffs) != 2 {
		t.Fatalf("expected 2 separate dataset PhaseDiffs (BaseMissing + CandMissing), got %+v", crossDS.PhaseDiffs)
	}
	for _, pd := range crossDS.PhaseDiffs {
		if pd.MemDeltaPct != 0 || (!pd.BaseMissing && !pd.CandMissing) {
			t.Fatalf("cross-dataset must not pair or compute MemDeltaPct, got %+v", pd)
		}
	}
	for _, f := range crossDS.Findings {
		if f.Metric == "steady_median_memory" {
			t.Fatalf("cross-dataset must not emit steady_median_memory alert: %+v", f)
		}
	}
}

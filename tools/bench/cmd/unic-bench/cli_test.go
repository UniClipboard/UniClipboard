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

func TestCLIRenderSingleMultiCompareAndErrors(t *testing.T) {
	dir := t.TempDir()
	macPath := filepath.Join(dir, "mac.json")
	winPath := filepath.Join(dir, "win.json")
	budgetPath := filepath.Join(dir, "budget.json")

	mk := func(id, osName, arch string, mem uint64, slope float64) *report.Report {
		return &report.Report{
			SchemaVersion:  report.SchemaVersion,
			RunID:          id,
			GeneratedAtUTC: "2026-11-27T00:00:00Z",
			Track:          "production",
			Preset:         "standard",
			Environment:    report.EnvironmentInfo{OS: osName, HostArch: arch, LogicalCPUs: 8},
			Scenarios: []report.ScenarioResult{
				{
					ID:     "bg-idle-normal",
					Track:  "production",
					Status: "PASS",
					Phases: []sampler.PhaseSummary{
						{Phase: "steady", SteadyMedianBytes: mem, CPUPercentMean: 1.0},
					},
					LeakCheck: &report.LeakSummary{
						Metric:             "qp_warm_post_close_primary_memory",
						Rounds:             5,
						SlopeBytesPerRound: slope,
						ValuesBytes:        []uint64{mem, mem, mem, mem, mem},
						Passed:             slope <= (1 << 20),
					},
				},
			},
		}
	}

	if err := report.WriteJSON(macPath, mk("mac-base", "darwin", "arm64", 80<<20, -100<<10)); err != nil {
		t.Fatal(err)
	}
	if err := report.WriteJSON(winPath, mk("win-cand", "windows", "arm64", 120<<20, 2<<20)); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(budgetPath, []byte(`{"schema_version":1,"scenarios":{"bg-idle-normal":{"max_primary_memory_bytes":100000000}}}`), 0o600); err != nil {
		t.Fatal(err)
	}

	// 1. Single report without --out writes HTML to stdout and creates no file in cwd or input dir.
	wd, err := os.Getwd()
	if err != nil {
		t.Fatal(err)
	}
	tmpCwd := t.TempDir()
	if err := os.Chdir(tmpCwd); err != nil {
		t.Fatal(err)
	}
	defer func() { _ = os.Chdir(wd) }()

	var stdout, stderr bytes.Buffer
	if code := runCLI(context.Background(), []string{"render", "--in", macPath}, &stdout, &stderr); code != exitOK {
		t.Fatalf("render single stdout exit=%d stderr=%s", code, stderr.String())
	}
	if !strings.Contains(stdout.String(), "<!DOCTYPE html>") || !strings.Contains(stdout.String(), "mac-base") {
		t.Fatalf("expected HTML on stdout when --out is omitted")
	}
	if _, err := os.Stat(filepath.Join(dir, "report.html")); !os.IsNotExist(err) {
		t.Fatalf("omitting --out must not write report.html in input dir")
	}
	if _, err := os.Stat(filepath.Join(tmpCwd, "dashboard.html")); !os.IsNotExist(err) {
		t.Fatalf("omitting --out must not write dashboard.html in cwd")
	}

	// 1b. Multi-report with --out - writes HTML to stdout and creates no dashboard.html.
	stdout.Reset()
	stderr.Reset()
	if code := runCLI(context.Background(), []string{"render", "--in", macPath, "--in", winPath, "--out", "-"}, &stdout, &stderr); code != exitOK {
		t.Fatalf("render multi stdout exit=%d stderr=%s", code, stderr.String())
	}
	if !strings.Contains(stdout.String(), "<!DOCTYPE html>") || !strings.Contains(stdout.String(), "win-cand") {
		t.Fatalf("expected merged HTML on stdout with --out -")
	}
	if _, err := os.Stat(filepath.Join(tmpCwd, "dashboard.html")); !os.IsNotExist(err) {
		t.Fatalf("--out - must not write dashboard.html in cwd")
	}

	// 2. Multi-report cross-platform render via positional + --in and --compare + --budget to explicit --out.
	cmpOut := filepath.Join(dir, "sub", "compare.html")
	stdout.Reset()
	stderr.Reset()
	if code := runCLI(context.Background(), []string{
		"render", "--compare", "--budget", budgetPath, "--title", "Custom Compare",
		"--in", macPath, "--label", "Mac",
		winPath, "--label", "Win",
		"--out", cmpOut,
	}, &stdout, &stderr); code != exitOK {
		t.Fatalf("render compare exit=%d stdout=%s stderr=%s", code, stdout.String(), stderr.String())
	}
	cmpHTML, err := os.ReadFile(cmpOut)
	if err != nil {
		t.Fatalf("read compare.html: %v", err)
	}
	for _, want := range []string{"Custom Compare", "BUDGET_EXCEEDED", "cross_platform_warning", `"label":"Mac"`, `"label":"Win"`} {
		if !strings.Contains(string(cmpHTML), want) {
			t.Fatalf("compare.html missing %q", want)
		}
	}

	// 3. Usage error cases.
	for _, badArgs := range [][]string{
		{"render"},
		{"render", "--compare", "--in", macPath},
		{"render", "--in", macPath, "--label", "a", "--label", "b"},
	} {
		stdout.Reset()
		stderr.Reset()
		if code := runCLI(context.Background(), badArgs, &stdout, &stderr); code != exitUsage {
			t.Fatalf("render %v exit=%d, want %d", badArgs, code, exitUsage)
		}
	}
}

func TestCLIRenderInvalidUTF8BytesInReport(t *testing.T) {
	dir := t.TempDir()
	inPath := filepath.Join(dir, "invalid-utf8.json")
	outPath := filepath.Join(dir, "out.html")

	// Construct JSON containing raw invalid UTF-8 byte 0xFF between an HTTP URL and a local path,
	// plus a quoted local path containing raw 0xFF.
	raw := []byte(`{"schema_version":1,"run_id":"utf8-test","environment":{"os":"darwin","host_arch":"arm64"},"scenarios":[{"id":"url_suffix","status":"FAIL","error":"url=https://example.test/tmp/guide`)
	raw = append(raw, 0xFF)
	raw = append(raw, []byte(`path=/private/tmp/SyntheticFixture/private-probe.json"},{"id":"quoted_path","status":"FAIL","error":"open \"/tmp/bad`)...)
	raw = append(raw, 0xFF)
	raw = append(raw, []byte(`path/file\": denied"}]}`)...)
	if err := os.WriteFile(inPath, raw, 0o600); err != nil {
		t.Fatal(err)
	}

	var stdout, stderr bytes.Buffer
	if code := runCLI(context.Background(), []string{"render", "--in", inPath, "--out", outPath}, &stdout, &stderr); code != exitOK {
		t.Fatalf("render invalid UTF-8 exit=%d stdout=%s stderr=%s", code, stdout.String(), stderr.String())
	}

	htmlOut, err := os.ReadFile(outPath)
	if err != nil {
		t.Fatalf("read out.html: %v", err)
	}
	text := string(htmlOut)
	if strings.Contains(text, "SyntheticFixture") || strings.Contains(text, "private-probe.json") {
		t.Fatalf("rendered HTML leaked synthetic local path after invalid UTF-8 byte")
	}
	if !strings.Contains(text, `[omitted: contains local path; inspect local report.json]`) {
		t.Fatalf("rendered HTML missing omitted summary for invalid UTF-8 delimited path")
	}
	if !strings.Contains(text, `open \"[REDACTED_PATH]\": denied`) {
		t.Fatalf("rendered HTML missing [REDACTED_PATH] for quoted path with invalid UTF-8")
	}
}

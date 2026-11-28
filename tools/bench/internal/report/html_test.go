package report

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/uniclipboard/unic/tools/bench/internal/sampler"
)

func TestRenderHTMLSingleMultiAndSHA256SUMS(t *testing.T) {
	mac := sampleReport("mac-run", 80<<20, 1.5, 45.0, -500<<10)
	win := sampleReport("win-run", 95<<20, 2.0, 65.0, 2<<20)
	win.Environment.OS = "windows"
	win.Environment.HostArch = "arm64"
	win.Scenarios[0].Phases[0].PrivateCommitBytes = sampler.MetricStats{Count: 5, Median: 120 << 20}
	win.Scenarios[0].Phases[0].WorkingSetBytes = sampler.MetricStats{Count: 5, Median: 130 << 20}
	win.Scenarios[1].DerivedMetrics = map[string]float64{
		"warm_post_close_first_bytes":   140 << 20,
		"warm_post_close_last_bytes":    146 << 20,
		"warm_post_close_delta_bytes":   6 << 20,
		"warm_post_close_handles_delta": -2,
	}
	win.Scenarios[1].LeakCheck.ValuesBytes = []uint64{140 << 20, 142 << 20, 144 << 20, 146 << 20}

	dir := t.TempDir()
	htmlPath := filepath.Join(dir, "report.html")
	if err := WriteHTML(htmlPath, HTMLOptions{
		Reports: []*Report{mac},
	}); err != nil {
		t.Fatalf("WriteHTML single: %v", err)
	}
	if err := WriteJSON(filepath.Join(dir, "report.json"), mac); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, "report.md"), []byte(RenderMarkdown(mac)), 0o600); err != nil {
		t.Fatal(err)
	}
	sumsPath, err := WriteSHA256SUMS(dir, "report.json", "report.md", "report.html")
	if err != nil {
		t.Fatalf("WriteSHA256SUMS: %v", err)
	}
	sumsRaw, err := os.ReadFile(sumsPath)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(sumsRaw), "  report.html\n") {
		t.Fatalf("SHA256SUMS missing report.html:\n%s", string(sumsRaw))
	}

	multiHTML, err := RenderHTML(HTMLOptions{
		Title:   "Cross-Platform Board",
		Reports: []*Report{mac, win},
		Labels:  []string{"macOS ARM64", "Windows ARM64"},
	})
	if err != nil {
		t.Fatalf("RenderHTML multi: %v", err)
	}
	for _, mustContain := range []string{
		"Content-Security-Policy",
		"default-src 'none'",
		"Cross-Platform Board",
		"ri_phys_footprint",
		"PrivateWorkingSet",
		`"cross_platform":true`,
		"NOT_RUN",
		`id="unic-bench-data"`,
	} {
		if !strings.Contains(multiHTML, mustContain) {
			t.Fatalf("multi HTML missing %q", mustContain)
		}
	}
	sanitized := strings.ReplaceAll(strings.ToLower(multiHTML), "http://www.w3.org/2000/svg", "")
	for _, mustNotContain := range []string{"http://", "https://", "<script src=", "<link rel="} {
		if strings.Contains(sanitized, mustNotContain) {
			t.Fatalf("multi HTML contains forbidden external reference %q", mustNotContain)
		}
	}
}

func TestRenderHTMLXSSAndControlCharEscaping(t *testing.T) {
	evil := sampleReport("</script><script>alert(1)</script>", 50<<20, 1.0, 20.0, 0)
	evil.Environment.CPUModel = "CPU <!-- comment --> \u2028\u2029 <img src=x onerror=alert(1)>"
	evil.Binaries = map[string]BinaryInfo{
		"</script><b>bin</b>": {
			Path:       "/tmp/a&b",
			SHA256:     "deadbeef",
			HostArch:   "arm64",
			BinaryArch: "amd64",
			Emulated:   true,
		},
	}
	evil.Scenarios[0].Notes = []string{"line1\u2028line2\u2029</script>"}
	evil.Scenarios[0].Error = "<svg onload=alert(2)>"
	evil.Scenarios[0].Attribution = []sampler.AttributionSnapshot{
		{
			Phase:    "steady",
			Platform: "darwin",
			Processes: map[int]sampler.ProcessAttribution{
				100: {
					PID:  100,
					Role: "unicd",
					Categories: map[string]sampler.FootprintCategory{
						"</script><script>alert(3)</script>": {DirtyBytes: 1024},
					},
				},
			},
		},
	}

	out, err := RenderHTML(HTMLOptions{Reports: []*Report{evil}})
	if err != nil {
		t.Fatalf("RenderHTML evil: %v", err)
	}

	// Only our template's own script tags should exist.
	if strings.Count(strings.ToLower(out), "</script>") != 2 {
		t.Fatalf("unexpected </script> count=%d (possible script breakout)", strings.Count(strings.ToLower(out), "</script>"))
	}
	if strings.Contains(out, "\u2028") || strings.Contains(out, "\u2029") {
		t.Fatal("raw U+2028 or U+2029 leaked into HTML")
	}
	if !strings.Contains(out, `"has_emulated":true`) {
		t.Fatal("expected has_emulated:true when binary is emulated")
	}
}

func TestRenderHTMLCompareBudgetAndEdgeCases(t *testing.T) {
	base := &Report{
		SchemaVersion: SchemaVersion,
		RunID:         "base-mac",
		Track:         "production",
		Environment:   EnvironmentInfo{OS: "darwin", HostArch: "arm64"},
		Scenarios: []ScenarioResult{
			{
				ID:     "qp-lifecycle",
				Track:  "production",
				Status: "PASS",
				LeakCheck: &LeakSummary{
					Metric:             "qp_warm_post_close_primary_memory",
					Rounds:             1,
					SlopeBytesPerRound: 0,
					ValuesBytes:        []uint64{100 << 20},
					Passed:             true,
				},
			},
		},
	}
	cand := &Report{
		SchemaVersion: SchemaVersion,
		RunID:         "cand-win",
		Track:         "interactive",
		Environment:   EnvironmentInfo{OS: "windows", HostArch: "arm64"},
		Scenarios: []ScenarioResult{
			{
				ID:     "fg-close-reclaim",
				Track:  "interactive",
				Status: "PASS",
				Phases: []sampler.PhaseSummary{
					{Phase: "pre-open-bg", SteadyMedianBytes: 80 << 20},
					{Phase: "post-close-bg", SteadyMedianBytes: 82 << 20},
				},
				DerivedMetrics: map[string]float64{
					"open_median_bytes": 120 << 20,
				},
			},
		},
	}
	cmp, err := Compare(base, cand, CompareOptions{Thresholds: DefaultThresholds()})
	if err != nil {
		t.Fatalf("Compare: %v", err)
	}
	html, err := RenderHTML(HTMLOptions{
		Reports:    []*Report{base, cand},
		Comparison: cmp,
	})
	if err != nil {
		t.Fatalf("RenderHTML compare edge: %v", err)
	}
	if !strings.Contains(html, "cross_platform_warning") {
		t.Fatalf("expected cross_platform_warning for mismatched OS/track compare")
	}
	if !strings.Contains(html, "open_median_bytes") {
		t.Fatalf("expected derived open_median_bytes in HTML payload")
	}

	if _, err := RenderHTML(HTMLOptions{}); err == nil {
		t.Fatal("expected error for empty reports")
	}
	if _, err := RenderHTML(HTMLOptions{Reports: []*Report{nil}}); err == nil {
		t.Fatal("expected error for nil report")
	}
}

func TestRenderHTMLRedactsPrivateUserAndTempPaths(t *testing.T) {
	rep := sampleReport("run-/Users/alice/work", 80<<20, 1.0, 30.0, 0)
	rep.Binaries = map[string]BinaryInfo{
		"unic": {
			Path:       "/Users/alice/src/unic/.build/bin/unic",
			SHA256:     "abc123",
			HostArch:   "arm64",
			BinaryArch: "arm64",
		},
		"unic-win": {
			Path:       `C:\Users\bob\AppData\Local\Temp\unic-bench-123\bin\prod\unic-desktop.exe`,
			SHA256:     "def456",
			HostArch:   "arm64",
			BinaryArch: "arm64",
		},
		"unic-linux": {
			Path:       "/home/charlie/builds/unicd",
			SHA256:     "7890",
			HostArch:   "amd64",
			BinaryArch: "amd64",
		},
	}
	rep.Scenarios[0].Notes = []string{
		`binary failed at C:\Users\bob\secret\file.txt`,
		`socket in /Users/alice/.cache/unic.sock`,
		`temp profile in /tmp/with space/FixtureCustomer/project/probe.json: denied`,
		`quoted mac temp "/private/tmp/with space/FixtureCustomer/project/probe.json": denied`,
		`quoted single '/Users/alice/My Docs/FixtureCustomer/secret.txt' failed`,
		`quoted backtick ` + "`" + `C:\Windows\Temp\With Space\FixtureCustomer\out.log` + "`" + ` failed`,
		`windows temp in C:\Windows\Temp\unic-run\With Space\out.log`,
		`Reference https://benchmark.example/api/tmp/status and http://localhost/v1/tmp/check`,
	}
	rep.Scenarios[0].Error = `cannot open /private/tmp/with space/FixtureCustomer/project/result.sock`

	out, err := RenderHTML(HTMLOptions{Reports: []*Report{rep}})
	if err != nil {
		t.Fatalf("RenderHTML redact: %v", err)
	}
	for _, leak := range []string{
		"/Users/alice",
		`C:\Users\bob`,
		`C:\\Users\\bob`,
		"/home/charlie",
		"/tmp/with space",
		"/private/tmp/with space",
		`C:\Windows\Temp`,
		`C:\\Windows\\Temp`,
		"FixtureCustomer",
		"probe.json",
		"secret.txt",
		"out.log",
		"result.sock",
	} {
		if strings.Contains(out, leak) {
			t.Fatalf("HTML leaked private path fragment %q", leak)
		}
	}
	// Verify JSON-decoded payload directly preserves quoted redactions, omitted summary, and URLs.
	for _, noteCheck := range []string{
		`\"[REDACTED_PATH]\"`,
		`'[REDACTED_PATH]'`,
		"`[REDACTED_PATH]`",
		omittedLocalPathSummary,
		"https://benchmark.example/api/tmp/status",
		"http://localhost/v1/tmp/check",
	} {
		if !strings.Contains(out, noteCheck) {
			t.Fatalf("JSON payload missing %q", noteCheck)
		}
	}
	// Direct unit check on redactHomePaths for quoted, escaped-quote, unclosed-quote, unquoted,
	// fail-closed delimiter/apostrophe, and preserved URL cases.
	urlMatrix := []struct {
		name string
		in   string
		want string
	}{
		{"quoted_posix", `open "/tmp/with space/FixtureCustomer/project/probe.json": denied`, `open "[REDACTED_PATH]": denied`},
		{"escaped_double_quote", `open "/private/tmp/benchmark\" FixtureCustomer/project/probe.json": denied`, `open "[REDACTED_PATH]": denied`},
		{"escaped_single_quote", `open '/tmp/Team\'s Folder/probe.json': denied`, `open '[REDACTED_PATH]': denied`},
		{"unclosed_quote", `open "/tmp/unclosed FixtureCustomer/project/probe.json`, omittedLocalPathSummary},
		{"win_trailing_backslash", `open "C:\Users\bob\trailing-slash\"`, omittedLocalPathSummary},
		{"win_trailing_backslash_later_quote", `open "C:\Users\Alice\" and "retry".`, omittedLocalPathSummary},
		{"unquoted_space", `open /tmp/with space/FixtureCustomer/project/probe.json: denied`, omittedLocalPathSummary},
		{"url_plain", `https://example.test/tmp/guide`, `https://example.test/tmp/guide`},
		{"url_single_quoted", `'https://example.test/tmp/status'`, `'https://example.test/tmp/status'`},
		{"url_double_quoted", `"https://example.test/tmp/status"`, `"https://example.test/tmp/status"`},
		{"url_pct27", `https://example.test/team%27s/tmp/guide`, `https://example.test/team%27s/tmp/guide`},
		{"url_single_quoted_pct27", `'https://example.test/team%27s/tmp/guide'`, `'https://example.test/team%27s/tmp/guide'`},
		{"mixed_single_quote_apostrophe_and_path", `endpoint='https://example.test/team's/tmp/guide',path=/private/tmp/SyntheticFixture/private-probe.json`, omittedLocalPathSummary},
		{"mixed_single_quote_and_path", `endpoint='https://example.test/tmp/guide',path=/private/tmp/secret`, omittedLocalPathSummary},
		{"mixed_comma_and_path", `url=https://example.test/foo,path=/private/tmp/secret`, omittedLocalPathSummary},
		{"mixed_semicolon_and_path", `url=https://example.test/foo;path=/Users/alice/secret`, omittedLocalPathSummary},
		{"raw_apostrophe_url_fail_closed", `https://example.test/team's/tmp/guide`, omittedLocalPathSummary},
		{"wrapped_apostrophe_url_fail_closed", `Reference 'https://example.test/team's/tmp/guide'.`, omittedLocalPathSummary},
		{"unicode_nbsp_keyval_path", "url=https://example.test/tmp/guide\u00a0path=/private/tmp/secret", omittedLocalPathSummary},
		{"unicode_em_space_keyval_path", "url=https://example.test/tmp/guide\u2003path=/private/tmp/secret", omittedLocalPathSummary},
		{"unicode_nbsp_bare_path", "https://example.test/tmp/guide\u00a0/private/tmp/secret", omittedLocalPathSummary},
		{"unicode_zwsp_bare_path", "https://example.test/tmp/guide\u200b/private/tmp/secret", omittedLocalPathSummary},
		{"unicode_fullwidth_comma_path", "https://example.test/tmp/guide，/private/tmp/secret", omittedLocalPathSummary},
		{"unicode_posix_trailing_backslash_nbsp", "open \"/tmp/dir\".\u00a0Retry \"ok\"", `open "[REDACTED_PATH]".` + "\u00a0" + `Retry "ok"`},
		{"unicode_posix_escaped_quote_before_nbsp", "open \"/tmp/dir\\\".\u00a0Retry \"ok\"", omittedLocalPathSummary},
		{"unicode_prefix_url_preserved", "参考\u00a0https://example.test/tmp/guide，谢谢", "参考\u00a0https://example.test/tmp/guide，谢谢"},
		{"unicode_cjk_direct_prefix_url_preserved", "参考https://example.test/tmp/guide", "参考https://example.test/tmp/guide"},
		{"ufffd_keyval_path", "url=https://example.test/tmp/guide\ufffdpath=/private/tmp/secret", omittedLocalPathSummary},
		{"ufffd_bare_path", "https://example.test/tmp/guide\ufffd/private/tmp/secret", omittedLocalPathSummary},
		{"unicode_arrow_symbol_path", "https://example.test/tmp/guide\u2192/private/tmp/secret", omittedLocalPathSummary},
		{"ascii_pipe_delim_path", "https://example.test/tmp/guide|/private/tmp/secret", omittedLocalPathSummary},
		{"ascii_backslash_delim_path", `https://example.test/tmp/guide\/private/tmp/secret`, omittedLocalPathSummary},
		{"quoted_path_with_ufffd", "open \"/tmp/bad\ufffdpath/file\": denied", `open "[REDACTED_PATH]": denied`},
		{"posix_escaped_quote_before_ufffd", "open \"/tmp/dir\\\"\ufffdRetry \"ok\"", omittedLocalPathSummary},
	}
	for _, tc := range urlMatrix {
		if got := redactHomePaths(tc.in); got != tc.want {
			t.Fatalf("redactHomePaths(%s) = %q, want %q", tc.name, got, tc.want)
		}
	}
	for _, keep := range []string{`"path":"unic"`, `"path":"unic-desktop.exe"`, `"path":"unicd"`} {
		if !strings.Contains(out, keep) {
			t.Fatalf("expected sanitized basename %s in HTML", keep)
		}
	}
	// Ensure caller's Report struct was not mutated in place.
	if rep.Binaries["unic"].Path != "/Users/alice/src/unic/.build/bin/unic" {
		t.Fatalf("caller BinaryInfo.Path was mutated: %s", rep.Binaries["unic"].Path)
	}

	// Verify candidate-missing diffs serialize cand_missing:true cleanly into HTML payload.
	emptyCand := &Report{
		SchemaVersion: SchemaVersion,
		RunID:         "empty-cand",
		Track:         "production",
		Environment:   EnvironmentInfo{OS: "darwin", HostArch: "arm64"},
	}
	candMissingCmp, err := Compare(rep, emptyCand, CompareOptions{Thresholds: DefaultThresholds()})
	if err != nil {
		t.Fatalf("Compare cand missing: %v", err)
	}
	candMissingHTML, err := RenderHTML(HTMLOptions{
		Reports:    []*Report{rep, emptyCand},
		Comparison: candMissingCmp,
	})
	if err != nil {
		t.Fatalf("RenderHTML candMissing: %v", err)
	}
	if !strings.Contains(candMissingHTML, `"cand_missing":true`) {
		t.Fatal("expected cand_missing:true in HTML payload")
	}
}

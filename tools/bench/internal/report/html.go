package report

import (
	"bytes"
	_ "embed"
	"encoding/json"
	"errors"
	"fmt"
	"html/template"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"time"
	"unicode"
	"unicode/utf8"

	"github.com/uniclipboard/unic/tools/bench/internal/sampler"
)

var (
	//go:embed assets/dashboard.html.tmpl
	dashboardHTMLTemplateRaw string
	//go:embed assets/dashboard.css
	dashboardCSSRaw string
	//go:embed assets/dashboard.js
	dashboardJSRaw string

	dashboardTmpl = template.Must(template.New("dashboard").Parse(dashboardHTMLTemplateRaw))
)

// HTMLOptions configures offline single-file HTML report rendering.
type HTMLOptions struct {
	Title      string
	Reports    []*Report
	Labels     []string
	Comparison *CompareResult
	Budget     *BudgetFile
}

type htmlTemplateData struct {
	Title    string
	CSS      template.CSS
	JS       template.JS
	DataJSON template.JS
}

type htmlPayload struct {
	Title            string              `json:"title"`
	GeneratedAtUTC   string              `json:"generated_at_utc"`
	CrossPlatform    bool                `json:"cross_platform"`
	HasEmulated      bool                `json:"has_emulated"`
	BasisNotice      string              `json:"basis_notice"`
	Reports          []htmlReportEntry   `json:"reports"`
	Anomalies        []htmlAnomaly       `json:"anomalies"`
	Comparison       *htmlCompareSummary `json:"comparison,omitempty"`
	NotRunBoundaries []string            `json:"not_run_boundaries"`
}

type htmlReportEntry struct {
	Index             int     `json:"index"`
	Label             string  `json:"label"`
	Platform          string  `json:"platform"`
	PrimaryMetricName string  `json:"primary_metric_name"`
	Report            *Report `json:"report"`
}

type htmlAnomaly struct {
	Severity    string `json:"severity"`
	ReportIndex int    `json:"report_index"`
	ReportLabel string `json:"report_label"`
	Scenario    string `json:"scenario"`
	Track       string `json:"track,omitempty"`
	Dataset     string `json:"dataset,omitempty"`
	Title       string `json:"title"`
	Detail      string `json:"detail"`
}

type htmlCompareSummary struct {
	Result               *CompareResult `json:"result"`
	Budget               *BudgetFile    `json:"budget,omitempty"`
	CrossPlatformWarning string         `json:"cross_platform_warning,omitempty"`
}

// RenderHTML renders one or more Schema v1 benchmark reports into a self-contained
// offline HTML dashboard with zero external network dependencies.
func RenderHTML(opts HTMLOptions) (string, error) {
	if len(opts.Reports) == 0 {
		return "", errors.New("at least one report is required to render HTML")
	}
	for i, r := range opts.Reports {
		if r == nil {
			return "", fmt.Errorf("report at index %d is nil", i)
		}
	}

	payload := buildHTMLPayload(opts)
	safeJSON, err := marshalSafeJSON(payload)
	if err != nil {
		return "", fmt.Errorf("marshal dashboard payload: %w", err)
	}

	data := htmlTemplateData{
		Title:    payload.Title,
		CSS:      template.CSS(dashboardCSSRaw),
		JS:       template.JS(dashboardJSRaw),
		DataJSON: template.JS(safeJSON),
	}

	var buf bytes.Buffer
	if err := dashboardTmpl.Execute(&buf, data); err != nil {
		return "", fmt.Errorf("execute dashboard template: %w", err)
	}
	return buf.String(), nil
}

// WriteHTML renders opts and writes the self-contained HTML report to path.
func WriteHTML(path string, opts HTMLOptions) error {
	if strings.TrimSpace(path) == "" {
		return errors.New("output HTML path is required")
	}
	rendered, err := RenderHTML(opts)
	if err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return fmt.Errorf("create html output dir: %w", err)
	}
	if err := os.WriteFile(path, []byte(rendered), 0o600); err != nil {
		return fmt.Errorf("write html report %s: %w", path, err)
	}
	return nil
}

func buildHTMLPayload(opts HTMLOptions) htmlPayload {
	sanitizedReports := make([]*Report, len(opts.Reports))
	for i, rawReport := range opts.Reports {
		sanitizedReports[i] = sanitizeReportForHTML(rawReport)
	}
	sanitizedLabels := make([]string, len(opts.Labels))
	for i, l := range opts.Labels {
		sanitizedLabels[i] = redactHomePaths(l)
	}

	title := redactHomePaths(strings.TrimSpace(opts.Title))
	if title == "" {
		if len(sanitizedReports) == 1 {
			title = fmt.Sprintf("Desktop Resource Benchmark — %s", reportDisplayLabel(sanitizedReports[0], sanitizedLabels, 0))
		} else {
			title = "Desktop Resource Benchmark — Cross-Platform Dashboard"
		}
	}

	oses := map[string]bool{}
	entries := make([]htmlReportEntry, 0, len(sanitizedReports))
	var anomalies []htmlAnomaly
	hasEmulated := false

	for i, r := range sanitizedReports {
		label := reportDisplayLabel(r, sanitizedLabels, i)
		osName := strings.TrimSpace(r.Environment.OS)
		if osName != "" {
			oses[osName] = true
		}

		entries = append(entries, htmlReportEntry{
			Index:             i,
			Label:             label,
			Platform:          osName,
			PrimaryMetricName: primaryMetricNameForOS(osName),
			Report:            r,
		})

		if r.ClipboardIntegrity.Available && !r.ClipboardIntegrity.Unchanged {
			anomalies = append(anomalies, htmlAnomaly{
				Severity:    "CRITICAL",
				ReportIndex: i,
				ReportLabel: label,
				Scenario:    "clipboard-integrity",
				Title:       "System clipboard sequence mutated (F9)",
				Detail:      fmt.Sprintf("sequence %d -> %d", r.ClipboardIntegrity.BeforeSequence, r.ClipboardIntegrity.AfterSequence),
			})
		}

		binNames := make([]string, 0, len(r.Binaries))
		for k := range r.Binaries {
			binNames = append(binNames, k)
		}
		sort.Strings(binNames)
		for _, k := range binNames {
			bi := r.Binaries[k]
			if bi.Emulated {
				hasEmulated = true
				anomalies = append(anomalies, htmlAnomaly{
					Severity:    "WARN",
					ReportIndex: i,
					ReportLabel: label,
					Scenario:    "binaries",
					Title:       fmt.Sprintf("Emulated binary %s (%s on %s host)", k, bi.BinaryArch, bi.HostArch),
					Detail:      "Results reflect emulation overhead and are not native baseline measurements.",
				})
			}
		}

		for _, sc := range r.Scenarios {
			scName := sc.ID
			if sc.Track != "" {
				scName += " [" + sc.Track + "]"
			}
			scTrack := nonEmpty(sc.Track, r.Track)
			scDataset := nonEmpty(sc.Dataset, sc.Preset)
			if sc.Status == "FAIL" {
				anomalies = append(anomalies, htmlAnomaly{
					Severity:    "CRITICAL",
					ReportIndex: i,
					ReportLabel: label,
					Scenario:    scName,
					Track:       scTrack,
					Dataset:     scDataset,
					Title:       "Scenario failed",
					Detail:      nonEmpty(sc.Error, "scenario status is FAIL"),
				})
			}
			if sc.LeakCheck != nil && !sc.LeakCheck.Passed {
				slopeMiB := sc.LeakCheck.SlopeBytesPerRound / (1024.0 * 1024.0)
				detail := fmt.Sprintf("OLS slope %+.2f MiB/round across %d rounds exceeds threshold (+1.00 MiB/round)", slopeMiB, sc.LeakCheck.Rounds)
				if handlesDelta, ok := sc.DerivedMetrics["warm_post_close_handles_delta"]; ok {
					detail += fmt.Sprintf("; handles delta %.0f", handlesDelta)
				}
				anomalies = append(anomalies, htmlAnomaly{
					Severity:    "ALERT",
					ReportIndex: i,
					ReportLabel: label,
					Scenario:    scName,
					Track:       scTrack,
					Dataset:     scDataset,
					Title:       fmt.Sprintf("Leak check failed (%s)", sc.LeakCheck.Metric),
					Detail:      detail,
				})
			}
		}
	}

	crossPlatform := len(oses) > 1
	basisNotice := buildBasisNotice(oses, crossPlatform)

	var cmpSummary *htmlCompareSummary
	if opts.Comparison != nil {
		cmpCopy := sanitizeCompareForHTML(opts.Comparison)
		crossWarn := cmpCopy.IncompatibilityReason
		if crossWarn == "" {
			crossWarn = detectCompareMismatchWarning(sanitizedReports)
		}
		cmpSummary = &htmlCompareSummary{
			Result:               cmpCopy,
			Budget:               opts.Budget,
			CrossPlatformWarning: crossWarn,
		}
		candIdx := -1
		if len(entries) >= 2 {
			candIdx = len(entries) - 1
		}
		for _, f := range cmpCopy.Findings {
			loc := f.Scenario
			if f.Phase != "" {
				loc += "/" + f.Phase
			}
			anomalies = append(anomalies, htmlAnomaly{
				Severity:    string(f.Severity),
				ReportIndex: candIdx,
				ReportLabel: cmpCopy.CandidateRunID,
				Scenario:    loc,
				Title:       f.Metric,
				Detail:      f.Message,
			})
		}
	}

	genAt := sanitizedReports[0].GeneratedAtUTC
	if len(sanitizedReports) > 1 {
		genAt = time.Now().UTC().Format(time.RFC3339)
	}

	return htmlPayload{
		Title:            title,
		GeneratedAtUTC:   genAt,
		CrossPlatform:    crossPlatform,
		HasEmulated:      hasEmulated,
		BasisNotice:      basisNotice,
		Reports:          entries,
		Anomalies:        anomalies,
		Comparison:       cmpSummary,
		NotRunBoundaries: defaultNotRunBoundaries(),
	}
}

func reportDisplayLabel(r *Report, labels []string, idx int) string {
	if idx >= 0 && idx < len(labels) && strings.TrimSpace(labels[idx]) != "" {
		return strings.TrimSpace(labels[idx])
	}
	osArch := strings.TrimSpace(r.Environment.OS)
	if r.Environment.HostArch != "" {
		osArch += "/" + r.Environment.HostArch
	}
	runID := strings.TrimSpace(r.RunID)
	if runID == "" || runID == "out" {
		if osArch != "" {
			return osArch
		}
		return fmt.Sprintf("report-%d", idx+1)
	}
	return runID
}

func primaryMetricNameForOS(osName string) string {
	switch strings.ToLower(strings.TrimSpace(osName)) {
	case "darwin":
		return "ri_phys_footprint"
	case "windows":
		return "PrivateWorkingSet"
	default:
		return "PrimaryMemory"
	}
}

func buildBasisNotice(oses map[string]bool, crossPlatform bool) string {
	if crossPlatform {
		return "macOS measures kernel ri_phys_footprint (auxiliary: RSS, IdleWakeups/s, FDs) while Windows measures PrivateWorkingSet (auxiliary: PrivateCommit, WorkingSet with shared DLL overlap note, Handles/GDI/USER). Cross-platform charts show relative increments, reclaim ratios, and leak slopes side-by-side — do NOT subtract raw cross-OS byte totals directly."
	}
	if oses["darwin"] {
		return "macOS primary memory metric: kernel ri_phys_footprint (aligned with Activity Monitor Memory column). Auxiliary metrics: RSS, IdleWakeups/s, FDs, and footprint -j region attribution. Windows-only counters display as —."
	}
	if oses["windows"] {
		return "Windows primary memory metric: PrivateWorkingSet (aligned with Task Manager Memory column). Auxiliary metrics: PrivateCommit, WorkingSet (includes shared DLL overlap), Handles, GDI, USER, and VirtualQueryEx committed regions. macOS-only counters display as —."
	}
	return "Primary memory reflects platform-native headline physical memory; unavailable counters display as —."
}

const omittedLocalPathSummary = "[omitted: contains local path; inspect local report.json]"

const (
	unixLocalPathPrefix = `(?i)(?:/Users/|/home/|/(?:private/)?tmp/|/(?:private/)?var/folders/)`
	winLocalPathPrefix  = `(?i)[a-z]:(?:\\|/)+(?:users|windows(?:\\|/)+temp)(?:\\|/)+`
)

var (
	localPathAtStartPattern = regexp.MustCompile(`^(?:` + unixLocalPathPrefix + `|` + winLocalPathPrefix + `)`)
	unquotedPathPattern     = regexp.MustCompile(unixLocalPathPrefix + `|(?:^|[^A-Za-z0-9])` + winLocalPathPrefix)
)

func redactHomePaths(s string) string {
	if s == "" {
		return ""
	}

	// 1. Protect ordinary http(s):// URLs with strict fail-closed delimiter bounds.
	tokenPrefix := "__UNIC_URL_"
	for strings.Contains(s, tokenPrefix) {
		tokenPrefix += "X_"
	}
	masked, urls := protectHTTPURLs(s, tokenPrefix)

	// 2. Deterministically redact quoted local/temp paths (handling backslash-escaped quotes).
	redacted, ambiguous := redactQuotedLocalPaths(masked)
	if ambiguous {
		return omittedLocalPathSummary
	}

	// 3. Any remaining unquoted local/temp path prefix cannot be safely delimited across spaces;
	// replace the entire field with a safe summary.
	if unquotedPathPattern.MatchString(redacted) {
		return omittedLocalPathSummary
	}

	// 4. Restore protected URLs.
	for i, u := range urls {
		tok := fmt.Sprintf("%s%d__", tokenPrefix, i)
		redacted = strings.ReplaceAll(redacted, tok, u)
	}
	return redacted
}

func protectHTTPURLs(s, tokenPrefix string) (string, []string) {
	var out strings.Builder
	var urls []string
	i := 0
	for i < len(s) {
		schemeLen := matchHTTPSchemeAt(s, i)
		if schemeLen > 0 && isURLBoundaryBefore(s, i) {
			j := i + schemeLen
			for j < len(s) && isAllowedURLASCII(s[j]) {
				j++
			}
			// Trim trailing outer sentence punctuation (e.g. '.', ':', unmatched ')', ']', '}').
			for j > i+schemeLen && isTrailingURLPunctuation(s[i:j], s[j-1]) {
				j--
			}
			if j > i+schemeLen {
				tok := fmt.Sprintf("%s%d__", tokenPrefix, len(urls))
				urls = append(urls, s[i:j])
				out.WriteString(tok)
				i = j
				continue
			}
		}
		_, size := utf8.DecodeRuneInString(s[i:])
		if size <= 0 {
			size = 1
		}
		out.WriteString(s[i : i+size])
		i += size
	}
	return out.String(), urls
}

func isUnicodeDelimiterOrSpace(r rune) bool {
	return r <= ' ' ||
		r == 0x7f ||
		unicode.IsSpace(r) ||
		unicode.IsControl(r) ||
		unicode.Is(unicode.Cf, r)
}

// isAllowedURLASCII strictly allows RFC 3986 printable ASCII URL characters while
// excluding "'", ",", and ";" per fail-closed boundary rules, and terminating on all
// non-ASCII UTF-8 bytes (>= 0x80), control/whitespace bytes, and non-RFC3986 delimiters.
func isAllowedURLASCII(b byte) bool {
	if (b >= 'a' && b <= 'z') || (b >= 'A' && b <= 'Z') || (b >= '0' && b <= '9') {
		return true
	}
	switch b {
	case '-', '.', '_', '~', ':', '/', '?', '#', '[', ']', '@', '!', '$', '&', '(', ')', '*', '+', '=', '%':
		return true
	default:
		return false
	}
}

func isTrailingURLPunctuation(urlSlice string, b byte) bool {
	switch b {
	case '.', ',', ';', ':', '!', '?':
		return true
	case ')':
		return strings.Count(urlSlice, "(") < strings.Count(urlSlice, ")")
	case ']':
		return strings.Count(urlSlice, "[") < strings.Count(urlSlice, "]")
	case '}':
		return strings.Count(urlSlice, "{") < strings.Count(urlSlice, "}")
	default:
		return false
	}
}

func matchHTTPSchemeAt(s string, idx int) int {
	rem := s[idx:]
	if len(rem) >= 8 && strings.EqualFold(rem[:8], "https://") {
		return 8
	}
	if len(rem) >= 7 && strings.EqualFold(rem[:7], "http://") {
		return 7
	}
	return 0
}

func isURLBoundaryBefore(s string, idx int) bool {
	if idx == 0 {
		return true
	}
	r, _ := utf8.DecodeLastRuneInString(s[:idx])
	if r == utf8.RuneError {
		return false
	}
	if r >= 0x80 {
		return true
	}
	if isUnicodeDelimiterOrSpace(r) {
		return true
	}
	switch r {
	case '"', '\'', '`', '(', '[', '{', '<', ',', ';', '=', ':', '|':
		return true
	default:
		return false
	}
}

func isWindowsDrivePrefix(s string) bool {
	return len(s) >= 3 &&
		((s[0] >= 'a' && s[0] <= 'z') || (s[0] >= 'A' && s[0] <= 'Z')) &&
		s[1] == ':' &&
		(s[2] == '\\' || s[2] == '/')
}

func isAmbiguousPostEscapedQuote(afterQuote string) bool {
	if afterQuote == "" {
		return true
	}
	r, size := utf8.DecodeRuneInString(afterQuote)
	if r == utf8.RuneError || r < ' ' || r == 0x7f {
		return true
	}
	if r >= 0x80 && (isUnicodeDelimiterOrSpace(r) || unicode.IsPunct(r) || unicode.IsSymbol(r)) {
		return true
	}
	if r == '.' || r == ',' || r == ';' {
		rest := afterQuote[size:]
		if rest == "" {
			return true
		}
		nextR, _ := utf8.DecodeRuneInString(rest)
		if nextR == utf8.RuneError || isUnicodeDelimiterOrSpace(nextR) {
			return true
		}
	}
	return false
}

func redactQuotedLocalPaths(s string) (string, bool) {
	var out strings.Builder
	i := 0
	for i < len(s) {
		q := s[i]
		if (q == '"' || q == '\'' || q == '`') && i+1 < len(s) && localPathAtStartPattern.MatchString(s[i+1:]) {
			winDrive := isWindowsDrivePrefix(s[i+1:])
			j := i + 1
			closed := false
			for j < len(s) {
				r, size := utf8.DecodeRuneInString(s[j:])
				if r == utf8.RuneError && size == 1 {
					return "", true
				}
				if winDrive {
					// Backslashes in Windows drive paths are directory separators, so \" or \'
					// is ambiguous with a trailing slash before the closing quote.
					if r == '\\' && j+1 < len(s) && s[j+1] == q {
						return "", true
					}
					if r == rune(q) {
						closed = true
						break
					}
					j += size
					continue
				}
				if r == '\\' && q != '`' {
					if j+1 >= len(s) {
						return "", true
					}
					nextR, nextSize := utf8.DecodeRuneInString(s[j+1:])
					if nextR == utf8.RuneError && nextSize == 1 {
						return "", true
					}
					if nextR == rune(q) && isAmbiguousPostEscapedQuote(s[j+2:]) {
						return "", true
					}
					j += 1 + nextSize
					continue
				}
				if r == rune(q) {
					closed = true
					break
				}
				j += size
			}
			if !closed {
				return "", true
			}
			out.WriteByte(q)
			out.WriteString("[REDACTED_PATH]")
			out.WriteByte(q)
			i = j + 1
			continue
		}
		_, size := utf8.DecodeRuneInString(s[i:])
		if size <= 0 {
			size = 1
		}
		out.WriteString(s[i : i+size])
		i += size
	}
	return out.String(), false
}

func baseNameAnyOS(p string) string {
	trimmed := strings.TrimSpace(p)
	if trimmed == "" {
		return ""
	}
	normalized := strings.ReplaceAll(trimmed, `\`, "/")
	normalized = strings.TrimRight(normalized, "/")
	if normalized == "" {
		return ""
	}
	if idx := strings.LastIndex(normalized, "/"); idx >= 0 {
		return normalized[idx+1:]
	}
	return normalized
}

func sanitizeReportForHTML(src *Report) *Report {
	if src == nil {
		return nil
	}
	cp := *src
	cp.RunID = redactHomePaths(cp.RunID)
	cp.Environment.OSVersion = redactHomePaths(cp.Environment.OSVersion)
	cp.Environment.CPUModel = redactHomePaths(cp.Environment.CPUModel)
	cp.Environment.Hypervisor = redactHomePaths(cp.Environment.Hypervisor)
	cp.ClipboardIntegrity.Note = redactHomePaths(cp.ClipboardIntegrity.Note)

	if len(src.Binaries) > 0 {
		bins := make(map[string]BinaryInfo, len(src.Binaries))
		for k, bi := range src.Binaries {
			biCopy := bi
			biCopy.Path = baseNameAnyOS(bi.Path)
			if len(bi.BuildTags) > 0 {
				biCopy.BuildTags = append([]string(nil), bi.BuildTags...)
			}
			bins[redactHomePaths(k)] = biCopy
		}
		cp.Binaries = bins
	}

	if len(src.Scenarios) > 0 {
		scs := make([]ScenarioResult, len(src.Scenarios))
		for i, sc := range src.Scenarios {
			scCopy := sc
			scCopy.Error = redactHomePaths(sc.Error)
			if len(sc.Notes) > 0 {
				notes := make([]string, len(sc.Notes))
				for j, n := range sc.Notes {
					notes[j] = redactHomePaths(n)
				}
				scCopy.Notes = notes
			}
			if len(sc.Phases) > 0 {
				phases := append([]sampler.PhaseSummary(nil), sc.Phases...)
				for j := range phases {
					phases[j].WorkingSetOverlapNote = redactHomePaths(phases[j].WorkingSetOverlapNote)
				}
				scCopy.Phases = phases
			}
			if len(sc.Latencies) > 0 {
				lats := make(map[string]LatencyMetric, len(sc.Latencies))
				for lk, lv := range sc.Latencies {
					lvCopy := lv
					lvCopy.Note = redactHomePaths(lv.Note)
					lats[lk] = lvCopy
				}
				scCopy.Latencies = lats
			}
			if len(sc.Attribution) > 0 {
				attrs := make([]sampler.AttributionSnapshot, len(sc.Attribution))
				for j, snap := range sc.Attribution {
					snapCopy := snap
					if len(snap.Warnings) > 0 {
						warns := make([]string, len(snap.Warnings))
						for wIdx, w := range snap.Warnings {
							warns[wIdx] = redactHomePaths(w)
						}
						snapCopy.Warnings = warns
					}
					if len(snap.Processes) > 0 {
						procs := make(map[int]sampler.ProcessAttribution, len(snap.Processes))
						for pid, pa := range snap.Processes {
							paCopy := pa
							paCopy.Note = redactHomePaths(pa.Note)
							procs[pid] = paCopy
						}
						snapCopy.Processes = procs
					}
					attrs[j] = snapCopy
				}
				scCopy.Attribution = attrs
			}
			scs[i] = scCopy
		}
		cp.Scenarios = scs
	}

	return &cp
}

func sanitizeCompareForHTML(src *CompareResult) *CompareResult {
	if src == nil {
		return nil
	}
	cp := *src
	cp.BaselineRunID = redactHomePaths(cp.BaselineRunID)
	cp.CandidateRunID = redactHomePaths(cp.CandidateRunID)
	cp.IncompatibilityReason = redactHomePaths(cp.IncompatibilityReason)
	if len(src.Findings) > 0 {
		findings := append([]CompareFinding(nil), src.Findings...)
		for i := range findings {
			findings[i].Message = redactHomePaths(findings[i].Message)
		}
		cp.Findings = findings
	}
	return &cp
}

func detectCompareMismatchWarning(reports []*Report) string {
	if len(reports) < 2 || reports[0] == nil || reports[1] == nil {
		return ""
	}
	base, cand := reports[0], reports[1]
	var reasons []string
	if base.Environment.OS != cand.Environment.OS {
		reasons = append(reasons, fmt.Sprintf("OS differs (%s vs %s)", base.Environment.OS, cand.Environment.OS))
	}
	if base.Track != "" && cand.Track != "" && base.Track != "both" && cand.Track != "both" && base.Track != cand.Track {
		reasons = append(reasons, fmt.Sprintf("track differs (%s vs %s)", base.Track, cand.Track))
	}
	baseEmu := anyBinaryEmulated(base)
	candEmu := anyBinaryEmulated(cand)
	if baseEmu != candEmu {
		reasons = append(reasons, fmt.Sprintf("emulation state differs (%t vs %t)", baseEmu, candEmu))
	}
	if len(reasons) == 0 {
		return ""
	}
	return "Comparing reports across mismatched baselines (" + strings.Join(reasons, "; ") + "). Interpret absolute deltas with caution."
}

func anyBinaryEmulated(r *Report) bool {
	if r == nil {
		return false
	}
	for _, bi := range r.Binaries {
		if bi.Emulated {
			return true
		}
	}
	return false
}

func defaultNotRunBoundaries() []string {
	return []string{
		"Physical windows/amd64 hardware baseline: NOT_RUN (scheduled via manual GitHub Actions windows-2025 matrix).",
		"Physical Intel macOS (darwin/amd64) hardware baseline: NOT_RUN (scheduled via manual GitHub Actions macos-15-intel matrix).",
		"Physical global hotkey driver delivery & display compositor first-frame photon latency: NOT_RUN (ipc_to_*_ms measures IPC/automation dispatch to readiness only).",
		"24-hour sleep/wake endurance & active remote sync (sync.enabled=true) long-haul network resource benchmarks: NOT_RUN.",
	}
}

var safeJSONReplacer = strings.NewReplacer(
	"<", `\u003c`,
	">", `\u003e`,
	"&", `\u0026`,
	"\u2028", `\u2028`,
	"\u2029", `\u2029`,
)

func marshalSafeJSON(v any) (string, error) {
	raw, err := json.Marshal(v)
	if err != nil {
		return "", err
	}
	return safeJSONReplacer.Replace(string(raw)), nil
}

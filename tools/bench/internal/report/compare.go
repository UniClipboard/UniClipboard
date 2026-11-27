package report

import (
	"encoding/json"
	"fmt"
	"math"
	"os"
	"sort"
	"strings"
)

// Thresholds defines relative regression alert limits (D4=A defaults).
type Thresholds struct {
	MemoryRelPct           float64 `json:"memory_rel_pct"`             // default +10%
	CPURelPct              float64 `json:"cpu_rel_pct"`                // default +20%
	LatencyDeltaMs         float64 `json:"latency_delta_ms"`           // default +100ms
	LeakSlopeBytesPerRound float64 `json:"leak_slope_bytes_per_round"` // default 1 MiB/round
}

// DefaultThresholds returns the Gate 1 D4=A regression thresholds.
func DefaultThresholds() Thresholds {
	return Thresholds{
		MemoryRelPct:           10.0,
		CPURelPct:              20.0,
		LatencyDeltaMs:         100.0,
		LeakSlopeBytesPerRound: 1 << 20, // 1 MiB/round
	}
}

// BudgetFile defines optional absolute resource budgets loaded via --budget.
type BudgetFile struct {
	SchemaVersion int                       `json:"schema_version,omitempty"`
	Scenarios     map[string]ScenarioBudget `json:"scenarios"`
}

// ScenarioBudget defines absolute ceilings for one scenario ID (0 disables a ceiling).
type ScenarioBudget struct {
	MaxPrimaryMemoryBytes     uint64             `json:"max_primary_memory_bytes,omitempty"`
	MaxPeakPrimaryMemoryBytes uint64             `json:"max_peak_primary_memory_bytes,omitempty"`
	MaxPrivateCommitBytes     uint64             `json:"max_private_commit_bytes,omitempty"`
	MaxCPUPercent             float64            `json:"max_cpu_percent,omitempty"`
	MaxIdleWakeupsPerSec      float64            `json:"max_idle_wakeups_per_sec,omitempty"`
	MaxLatencyP95Ms           map[string]float64 `json:"max_latency_p95_ms,omitempty"`
	MaxLeakSlopeBytesPerRound float64            `json:"max_leak_slope_bytes_per_round,omitempty"`
}

// LoadBudget parses a JSON budget file from path.
func LoadBudget(path string) (*BudgetFile, error) {
	raw, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("read budget %s: %w", path, err)
	}
	var bf BudgetFile
	if err := json.Unmarshal(raw, &bf); err != nil {
		return nil, fmt.Errorf("parse budget %s: %w", path, err)
	}
	if bf.Scenarios == nil {
		bf.Scenarios = map[string]ScenarioBudget{}
	}
	return &bf, nil
}

// CompareOptions configures report comparison.
type CompareOptions struct {
	Thresholds Thresholds
	Budget     *BudgetFile
}

// FindingSeverity classifies a comparison alert.
type FindingSeverity string

const (
	SeverityAlert          FindingSeverity = "ALERT"
	SeverityBudgetExceeded FindingSeverity = "BUDGET_EXCEEDED"
)

// CompareFinding records one regression or budget violation.
type CompareFinding struct {
	Severity  FindingSeverity `json:"severity"`
	Scenario  string          `json:"scenario"`
	Phase     string          `json:"phase,omitempty"`
	Metric    string          `json:"metric"`
	Baseline  string          `json:"baseline,omitempty"`
	Candidate string          `json:"candidate"`
	Delta     string          `json:"delta,omitempty"`
	Threshold string          `json:"threshold"`
	Message   string          `json:"message"`
}

// PhaseDiff captures baseline vs candidate metrics for one scenario phase.
type PhaseDiff struct {
	Scenario         string  `json:"scenario"`
	Track            string  `json:"track,omitempty"`
	Dataset          string  `json:"dataset,omitempty"`
	Phase            string  `json:"phase"`
	BaseMemMedian    uint64  `json:"base_mem_median"`
	CandMemMedian    uint64  `json:"cand_mem_median"`
	MemDeltaPct      float64 `json:"mem_delta_pct"`
	BaseCPUPercent   float64 `json:"base_cpu_percent"`
	CandCPUPercent   float64 `json:"cand_cpu_percent"`
	CPUDeltaPct      float64 `json:"cpu_delta_pct"`
	BaseLeakSlopeBPR float64 `json:"base_leak_slope_bpr,omitempty"`
	CandLeakSlopeBPR float64 `json:"cand_leak_slope_bpr,omitempty"`
}

// LatencyDiff captures baseline vs candidate P95 latency for one metric.
type LatencyDiff struct {
	Scenario string  `json:"scenario"`
	Track    string  `json:"track,omitempty"`
	Dataset  string  `json:"dataset,omitempty"`
	Metric   string  `json:"metric"`
	BaseP95  float64 `json:"base_p95_ms"`
	CandP95  float64 `json:"cand_p95_ms"`
	DeltaMs  float64 `json:"delta_ms"`
}

// CompareResult holds all diffs and threshold/budget findings.
type CompareResult struct {
	BaselineRunID  string           `json:"baseline_run_id"`
	CandidateRunID string           `json:"candidate_run_id"`
	Thresholds     Thresholds       `json:"thresholds"`
	PhaseDiffs     []PhaseDiff      `json:"phase_diffs"`
	LatencyDiffs   []LatencyDiff    `json:"latency_diffs"`
	Findings       []CompareFinding `json:"findings"`
}

// HasRegressions reports whether any regression alert or budget violation occurred.
func (r *CompareResult) HasRegressions() bool {
	return r != nil && len(r.Findings) > 0
}

// Compare compares candidate against baseline using relative regression thresholds
// and optional absolute budget ceilings.
func Compare(base, cand *Report, opts CompareOptions) (*CompareResult, error) {
	if base == nil || cand == nil {
		return nil, fmt.Errorf("both baseline and candidate reports are required")
	}
	th := opts.Thresholds
	if th.MemoryRelPct <= 0 {
		th.MemoryRelPct = DefaultThresholds().MemoryRelPct
	}
	if th.CPURelPct <= 0 {
		th.CPURelPct = DefaultThresholds().CPURelPct
	}
	if th.LatencyDeltaMs <= 0 {
		th.LatencyDeltaMs = DefaultThresholds().LatencyDeltaMs
	}
	if th.LeakSlopeBytesPerRound <= 0 {
		th.LeakSlopeBytesPerRound = DefaultThresholds().LeakSlopeBytesPerRound
	}

	res := &CompareResult{
		BaselineRunID:  base.RunID,
		CandidateRunID: cand.RunID,
		Thresholds:     th,
	}

	baseScenarios := make(map[string]ScenarioResult, len(base.Scenarios))
	for _, sc := range base.Scenarios {
		baseScenarios[scenarioCompareKey(sc)] = sc
	}

	for _, csc := range cand.Scenarios {
		bsc, hasBase := baseScenarios[scenarioCompareKey(csc)]
		if !hasBase && (csc.Dataset != "" || csc.Track != "") {
			// Fallback to bare ID when comparing against older single-dataset reports.
			for _, sc := range base.Scenarios {
				if sc.ID == csc.ID {
					bsc, hasBase = sc, true
					break
				}
			}
		}
		basePhases := map[string]int{}
		if hasBase {
			for i, ph := range bsc.Phases {
				basePhases[ph.Phase] = i
			}
		}

		var budget ScenarioBudget
		hasBudget := false
		if opts.Budget != nil {
			budget, hasBudget = opts.Budget.Scenarios[csc.ID]
		}

		for _, cph := range csc.Phases {
			var diff PhaseDiff
			diff.Scenario = csc.ID
			diff.Track = csc.Track
			diff.Dataset = csc.Dataset
			diff.Phase = cph.Phase
			diff.CandMemMedian = cph.SteadyMedianBytes
			diff.CandCPUPercent = cph.CPUPercentMean
			diff.CandLeakSlopeBPR = cph.LeakSlopeBytesPerRound

			if idx, ok := basePhases[cph.Phase]; ok {
				bph := bsc.Phases[idx]
				diff.BaseMemMedian = bph.SteadyMedianBytes
				diff.BaseCPUPercent = bph.CPUPercentMean
				diff.BaseLeakSlopeBPR = bph.LeakSlopeBytesPerRound
				diff.MemDeltaPct = relPct(float64(bph.SteadyMedianBytes), float64(cph.SteadyMedianBytes))
				diff.CPUDeltaPct = relPct(bph.CPUPercentMean, cph.CPUPercentMean)

				if bph.SteadyMedianBytes > 0 && diff.MemDeltaPct > th.MemoryRelPct {
					res.Findings = append(res.Findings, CompareFinding{
						Severity:  SeverityAlert,
						Scenario:  csc.ID,
						Phase:     cph.Phase,
						Metric:    "steady_median_memory",
						Baseline:  formatBytes(bph.SteadyMedianBytes),
						Candidate: formatBytes(cph.SteadyMedianBytes),
						Delta:     fmt.Sprintf("+%.1f%%", diff.MemDeltaPct),
						Threshold: fmt.Sprintf("+%.1f%%", th.MemoryRelPct),
						Message:   fmt.Sprintf("steady median memory regressed by +%.1f%% (> +%.1f%%)", diff.MemDeltaPct, th.MemoryRelPct),
					})
				}
				if bph.CPUPercentMean > 0.05 && diff.CPUDeltaPct > th.CPURelPct {
					res.Findings = append(res.Findings, CompareFinding{
						Severity:  SeverityAlert,
						Scenario:  csc.ID,
						Phase:     cph.Phase,
						Metric:    "cpu_percent_mean",
						Baseline:  fmt.Sprintf("%.2f%%", bph.CPUPercentMean),
						Candidate: fmt.Sprintf("%.2f%%", cph.CPUPercentMean),
						Delta:     fmt.Sprintf("+%.1f%%", diff.CPUDeltaPct),
						Threshold: fmt.Sprintf("+%.1f%%", th.CPURelPct),
						Message:   fmt.Sprintf("CPU utilization regressed by +%.1f%% (> +%.1f%%)", diff.CPUDeltaPct, th.CPURelPct),
					})
				}
			}

			if cph.LeakSlopeBytesPerRound > th.LeakSlopeBytesPerRound {
				res.Findings = append(res.Findings, CompareFinding{
					Severity:  SeverityAlert,
					Scenario:  csc.ID,
					Phase:     cph.Phase,
					Metric:    "leak_slope_bytes_per_round",
					Candidate: fmt.Sprintf("%.2f MiB/round", cph.LeakSlopeBytesPerRound/(1024*1024)),
					Threshold: fmt.Sprintf("%.2f MiB/round", th.LeakSlopeBytesPerRound/(1024*1024)),
					Message:   fmt.Sprintf("phase leak slope %.2f MiB/round exceeds %.2f MiB/round", cph.LeakSlopeBytesPerRound/(1024*1024), th.LeakSlopeBytesPerRound/(1024*1024)),
				})
			}

			if hasBudget {
				if budget.MaxPrimaryMemoryBytes > 0 && cph.SteadyMedianBytes > budget.MaxPrimaryMemoryBytes {
					res.Findings = append(res.Findings, CompareFinding{
						Severity:  SeverityBudgetExceeded,
						Scenario:  csc.ID,
						Phase:     cph.Phase,
						Metric:    "steady_median_memory",
						Candidate: formatBytes(cph.SteadyMedianBytes),
						Threshold: formatBytes(budget.MaxPrimaryMemoryBytes),
						Message:   fmt.Sprintf("steady median memory %s exceeds budget %s", formatBytes(cph.SteadyMedianBytes), formatBytes(budget.MaxPrimaryMemoryBytes)),
					})
				}
				if budget.MaxPeakPrimaryMemoryBytes > 0 && cph.PeakPrimaryMemoryBytes > budget.MaxPeakPrimaryMemoryBytes {
					res.Findings = append(res.Findings, CompareFinding{
						Severity:  SeverityBudgetExceeded,
						Scenario:  csc.ID,
						Phase:     cph.Phase,
						Metric:    "peak_primary_memory",
						Candidate: formatBytes(cph.PeakPrimaryMemoryBytes),
						Threshold: formatBytes(budget.MaxPeakPrimaryMemoryBytes),
						Message:   fmt.Sprintf("peak primary memory %s exceeds budget %s", formatBytes(cph.PeakPrimaryMemoryBytes), formatBytes(budget.MaxPeakPrimaryMemoryBytes)),
					})
				}
				if budget.MaxPrivateCommitBytes > 0 && uint64(math.Round(cph.PrivateCommitBytes.Median)) > budget.MaxPrivateCommitBytes {
					candCommit := uint64(math.Round(cph.PrivateCommitBytes.Median))
					res.Findings = append(res.Findings, CompareFinding{
						Severity:  SeverityBudgetExceeded,
						Scenario:  csc.ID,
						Phase:     cph.Phase,
						Metric:    "private_commit_median",
						Candidate: formatBytes(candCommit),
						Threshold: formatBytes(budget.MaxPrivateCommitBytes),
						Message:   fmt.Sprintf("private commit median %s exceeds budget %s", formatBytes(candCommit), formatBytes(budget.MaxPrivateCommitBytes)),
					})
				}
				if budget.MaxCPUPercent > 0 && cph.CPUPercentMean > budget.MaxCPUPercent {
					res.Findings = append(res.Findings, CompareFinding{
						Severity:  SeverityBudgetExceeded,
						Scenario:  csc.ID,
						Phase:     cph.Phase,
						Metric:    "cpu_percent_mean",
						Candidate: fmt.Sprintf("%.2f%%", cph.CPUPercentMean),
						Threshold: fmt.Sprintf("%.2f%%", budget.MaxCPUPercent),
						Message:   fmt.Sprintf("CPU %.2f%% exceeds budget %.2f%%", cph.CPUPercentMean, budget.MaxCPUPercent),
					})
				}
				if budget.MaxIdleWakeupsPerSec > 0 && cph.IdleWakeupsPerSec > budget.MaxIdleWakeupsPerSec {
					res.Findings = append(res.Findings, CompareFinding{
						Severity:  SeverityBudgetExceeded,
						Scenario:  csc.ID,
						Phase:     cph.Phase,
						Metric:    "idle_wakeups_per_sec",
						Candidate: fmt.Sprintf("%.2f/s", cph.IdleWakeupsPerSec),
						Threshold: fmt.Sprintf("%.2f/s", budget.MaxIdleWakeupsPerSec),
						Message:   fmt.Sprintf("idle wakeups %.2f/s exceeds budget %.2f/s", cph.IdleWakeupsPerSec, budget.MaxIdleWakeupsPerSec),
					})
				}
			}

			res.PhaseDiffs = append(res.PhaseDiffs, diff)
		}

		// Compare scenario-level latencies.
		latKeys := make([]string, 0, len(csc.Latencies))
		for k := range csc.Latencies {
			latKeys = append(latKeys, k)
		}
		sort.Strings(latKeys)
		for _, k := range latKeys {
			clat := csc.Latencies[k]
			ld := LatencyDiff{
				Scenario: csc.ID,
				Track:    csc.Track,
				Dataset:  csc.Dataset,
				Metric:   k,
				CandP95:  clat.P95Ms,
			}
			if hasBase {
				if blat, ok := bsc.Latencies[k]; ok {
					ld.BaseP95 = blat.P95Ms
					ld.DeltaMs = clat.P95Ms - blat.P95Ms
					if ld.DeltaMs > th.LatencyDeltaMs {
						res.Findings = append(res.Findings, CompareFinding{
							Severity:  SeverityAlert,
							Scenario:  csc.ID,
							Metric:    k + ".p95_ms",
							Baseline:  fmt.Sprintf("%.1f ms", blat.P95Ms),
							Candidate: fmt.Sprintf("%.1f ms", clat.P95Ms),
							Delta:     fmt.Sprintf("+%.1f ms", ld.DeltaMs),
							Threshold: fmt.Sprintf("+%.1f ms", th.LatencyDeltaMs),
							Message:   fmt.Sprintf("%s P95 regressed by +%.1f ms (> +%.1f ms)", k, ld.DeltaMs, th.LatencyDeltaMs),
						})
					}
				}
			}
			if hasBudget {
				if limit, ok := budget.MaxLatencyP95Ms[k]; ok && limit > 0 && clat.P95Ms > limit {
					res.Findings = append(res.Findings, CompareFinding{
						Severity:  SeverityBudgetExceeded,
						Scenario:  csc.ID,
						Metric:    k + ".p95_ms",
						Candidate: fmt.Sprintf("%.1f ms", clat.P95Ms),
						Threshold: fmt.Sprintf("%.1f ms", limit),
						Message:   fmt.Sprintf("%s P95 %.1f ms exceeds budget %.1f ms", k, clat.P95Ms, limit),
					})
				}
			}
			res.LatencyDiffs = append(res.LatencyDiffs, ld)
		}

		// Scenario-level multi-round leak check.
		if csc.LeakCheck != nil {
			slope := csc.LeakCheck.SlopeBytesPerRound
			if slope > th.LeakSlopeBytesPerRound {
				res.Findings = append(res.Findings, CompareFinding{
					Severity:  SeverityAlert,
					Scenario:  csc.ID,
					Metric:    csc.LeakCheck.Metric,
					Candidate: fmt.Sprintf("%.2f MiB/round", slope/(1024*1024)),
					Threshold: fmt.Sprintf("%.2f MiB/round", th.LeakSlopeBytesPerRound/(1024*1024)),
					Message:   fmt.Sprintf("leak slope %.2f MiB/round exceeds %.2f MiB/round", slope/(1024*1024), th.LeakSlopeBytesPerRound/(1024*1024)),
				})
			}
			if hasBudget && budget.MaxLeakSlopeBytesPerRound > 0 && slope > budget.MaxLeakSlopeBytesPerRound {
				res.Findings = append(res.Findings, CompareFinding{
					Severity:  SeverityBudgetExceeded,
					Scenario:  csc.ID,
					Metric:    csc.LeakCheck.Metric,
					Candidate: fmt.Sprintf("%.2f MiB/round", slope/(1024*1024)),
					Threshold: fmt.Sprintf("%.2f MiB/round", budget.MaxLeakSlopeBytesPerRound/(1024*1024)),
					Message:   fmt.Sprintf("leak slope %.2f MiB/round exceeds budget %.2f MiB/round", slope/(1024*1024), budget.MaxLeakSlopeBytesPerRound/(1024*1024)),
				})
			}
		}
	}

	return res, nil
}

// RenderCompareMarkdown renders a human-readable comparison table and alert list.
func RenderCompareMarkdown(res *CompareResult) string {
	if res == nil {
		return ""
	}
	var b strings.Builder
	fmt.Fprintf(&b, "# Benchmark Comparison\n\n")
	fmt.Fprintf(&b, "- **Baseline**: `%s`\n", res.BaselineRunID)
	fmt.Fprintf(&b, "- **Candidate**: `%s`\n", res.CandidateRunID)
	fmt.Fprintf(&b, "- **Status**: %s\n\n", map[bool]string{true: "REGRESSION / BUDGET ALERT", false: "OK"}[res.HasRegressions()])

	if len(res.PhaseDiffs) > 0 {
		fmt.Fprintf(&b, "## Memory & CPU Diff\n\n")
		fmt.Fprintf(&b, "| Scenario | Track | Dataset | Phase | Base Mem | Cand Mem | Mem Delta | Base CPU | Cand CPU | CPU Delta |\n")
		fmt.Fprintf(&b, "|---|---|---|---|---:|---:|---:|---:|---:|---:|\n")
		for _, pd := range res.PhaseDiffs {
			fmt.Fprintf(&b, "| `%s` | %s | %s | `%s` | %s | %s | %+.1f%% | %.2f%% | %.2f%% | %+.1f%% |\n",
				pd.Scenario, cellOrDash(pd.Track), cellOrDash(pd.Dataset), pd.Phase,
				formatBytes(pd.BaseMemMedian), formatBytes(pd.CandMemMedian), pd.MemDeltaPct,
				pd.BaseCPUPercent, pd.CandCPUPercent, pd.CPUDeltaPct,
			)
		}
		fmt.Fprintln(&b)
	}

	if len(res.LatencyDiffs) > 0 {
		fmt.Fprintf(&b, "## Latency P95 Diff\n\n")
		fmt.Fprintf(&b, "| Scenario | Track | Dataset | Metric | Base P95 (ms) | Cand P95 (ms) | Delta (ms) |\n")
		fmt.Fprintf(&b, "|---|---|---|---|---:|---:|---:|\n")
		for _, ld := range res.LatencyDiffs {
			fmt.Fprintf(&b, "| `%s` | %s | %s | `%s` | %.1f | %.1f | %+.1f |\n",
				ld.Scenario, cellOrDash(ld.Track), cellOrDash(ld.Dataset), ld.Metric, ld.BaseP95, ld.CandP95, ld.DeltaMs)
		}
		fmt.Fprintln(&b)
	}

	if len(res.Findings) > 0 {
		fmt.Fprintf(&b, "## Findings (%d)\n\n", len(res.Findings))
		for _, f := range res.Findings {
			loc := f.Scenario
			if f.Phase != "" {
				loc += "/" + f.Phase
			}
			fmt.Fprintf(&b, "- **[%s]** `%s` (`%s`): %s (threshold: %s)\n",
				f.Severity, loc, f.Metric, f.Message, f.Threshold)
		}
	} else {
		fmt.Fprintf(&b, "No regressions or budget violations detected.\n")
	}
	return b.String()
}

func cellOrDash(s string) string {
	if s == "" {
		return "-"
	}
	return "`" + s + "`"
}

func relPct(base, cand float64) float64 {
	if base <= 0 {
		return 0
	}
	return ((cand - base) / base) * 100.0
}

func scenarioCompareKey(sc ScenarioResult) string {
	return sc.ID + "|" + sc.Track + "|" + sc.Dataset
}

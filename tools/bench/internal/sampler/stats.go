package sampler

import (
	"math"
	"runtime"
	"slices"
	"sort"
)

// TrimWarmup splits samples into steady-state samples with ElapsedSec >= warmupSec
// and the count of trimmed warmup samples. If all samples fall within warmupSec,
// it falls back to returning all samples with trimmed=0 so callers never get an
// empty slice unintentionally when samples exist.
func TrimWarmup(samples []GroupSample, warmupSec float64) (steady []GroupSample, trimmed int) {
	if len(samples) == 0 || warmupSec <= 0 {
		return slices.Clone(samples), 0
	}
	idx := 0
	for idx < len(samples) && samples[idx].ElapsedSec < warmupSec {
		idx++
	}
	if idx == len(samples) {
		return slices.Clone(samples), 0
	}
	return slices.Clone(samples[idx:]), idx
}

// TrimWarmupCount removes the leading n samples from vals.
func TrimWarmupCount(vals []float64, n int) []float64 {
	if n <= 0 || len(vals) == 0 {
		return slices.Clone(vals)
	}
	if n >= len(vals) {
		return nil
	}
	return slices.Clone(vals[n:])
}

// SummarizeValues computes min, max, mean, median, P95, and P99 over vals.
func SummarizeValues(vals []float64) MetricStats {
	if len(vals) == 0 {
		return MetricStats{}
	}
	sorted := slices.Clone(vals)
	sort.Float64s(sorted)

	var sum float64
	for _, v := range sorted {
		sum += v
	}
	return MetricStats{
		Count:  len(sorted),
		Min:    sorted[0],
		Max:    sorted[len(sorted)-1],
		Mean:   sum / float64(len(sorted)),
		Median: quantileSorted(sorted, 0.50),
		P95:    quantileSorted(sorted, 0.95),
		P99:    quantileSorted(sorted, 0.99),
	}
}

func quantileSorted(sorted []float64, q float64) float64 {
	n := len(sorted)
	if n == 0 {
		return 0
	}
	if n == 1 || q <= 0 {
		return sorted[0]
	}
	if q >= 1 {
		return sorted[n-1]
	}
	pos := q * float64(n-1)
	lo := int(math.Floor(pos))
	hi := int(math.Ceil(pos))
	if lo == hi {
		return sorted[lo]
	}
	frac := pos - float64(lo)
	return sorted[lo]*(1-frac) + sorted[hi]*frac
}

// LinearRegressionSlope computes ordinary least-squares slope and intercept for (xs, ys).
// Returns (0, 0) if fewer than 2 points exist or xs has zero variance.
func LinearRegressionSlope(xs, ys []float64) (slope, intercept float64) {
	n := min(len(xs), len(ys))
	if n < 2 {
		return 0, 0
	}
	var sumX, sumY float64
	for i := 0; i < n; i++ {
		sumX += xs[i]
		sumY += ys[i]
	}
	meanX := sumX / float64(n)
	meanY := sumY / float64(n)

	var num, den float64
	for i := 0; i < n; i++ {
		dx := xs[i] - meanX
		dy := ys[i] - meanY
		num += dx * dy
		den += dx * dx
	}
	if den == 0 {
		return 0, meanY
	}
	slope = num / den
	intercept = meanY - slope*meanX
	return slope, intercept
}

// SummarizePhase computes steady-state distributions, early transient peak,
// CPU usage, wakeup rates, and leak regression slope across a phase's GroupSamples.
func SummarizePhase(phase string, samples []GroupSample, expectedRoles []string, warmupSec, earlyPeakWindowSec float64) PhaseSummary {
	out := PhaseSummary{
		Phase:         phase,
		ExpectedRoles: slices.Clone(expectedRoles),
		ByRole:        make(map[string]RolePhaseSummary),
	}
	if len(samples) == 0 {
		out.MissingRoles = slices.Clone(expectedRoles)
		return out
	}

	if runtime.GOOS == "windows" {
		out.WorkingSetOverlapNote = "Sum of per-process WorkingSet64 may double-count shared pages; use primary_memory_bytes (PrivateWorkingSetSize) and private_commit_bytes for cross-process sums."
	}

	missingSet := map[string]bool{}
	var overallPeak, earlyPeak uint64
	for _, gs := range samples {
		if gs.TotalPrimaryMemoryBytes > overallPeak {
			overallPeak = gs.TotalPrimaryMemoryBytes
		}
		if earlyPeakWindowSec <= 0 || gs.ElapsedSec <= earlyPeakWindowSec {
			if gs.TotalPrimaryMemoryBytes > earlyPeak {
				earlyPeak = gs.TotalPrimaryMemoryBytes
			}
		}
		for _, r := range gs.MissingRoles {
			missingSet[r] = true
		}
	}
	for _, r := range expectedRoles {
		if missingSet[r] {
			out.MissingRoles = append(out.MissingRoles, r)
		}
	}

	steady, trimmed := TrimWarmup(samples, warmupSec)
	out.WarmupTrimmedSamples = trimmed
	out.SampleCount = len(steady)
	out.PeakPrimaryMemoryBytes = overallPeak
	out.EarlyPeakBytes = earlyPeak

	wallSec := wallDurationSec(steady)
	out.DurationSec = wallSec

	primaryVals := make([]float64, 0, len(steady))
	commitVals := make([]float64, 0, len(steady))
	wsVals := make([]float64, 0, len(steady))
	xs := make([]float64, 0, len(steady))
	hasCommit := false
	hasWS := false

	for i, gs := range steady {
		primaryVals = append(primaryVals, float64(gs.TotalPrimaryMemoryBytes))
		xs = append(xs, float64(i))
		if gs.TotalPrivateCommitBytes > 0 {
			hasCommit = true
		}
		commitVals = append(commitVals, float64(gs.TotalPrivateCommitBytes))
		if gs.TotalWorkingSetBytes > 0 {
			hasWS = true
		}
		wsVals = append(wsVals, float64(gs.TotalWorkingSetBytes))
	}

	out.PrimaryMemoryBytes = SummarizeValues(primaryVals)
	out.SteadyMedianBytes = uint64(math.Round(out.PrimaryMemoryBytes.Median))
	out.SteadyP95Bytes = uint64(math.Round(out.PrimaryMemoryBytes.P95))
	if hasCommit {
		out.PrivateCommitBytes = SummarizeValues(commitVals)
	}
	if hasWS {
		out.WorkingSetBytes = SummarizeValues(wsVals)
	}
	out.LeakSlopeBytesPerSample, _ = LinearRegressionSlope(xs, primaryVals)

	roleNames := collectRoleNames(steady, expectedRoles)
	var totalCPUSeconds float64
	var totalIdleDelta, totalInterruptDelta uint64
	for _, role := range roleNames {
		rs := summarizeRolePhase(role, steady, wallSec)
		out.ByRole[role] = rs
		totalCPUSeconds += rs.CPUSeconds
	}
	if len(steady) >= 2 && wallSec > 0 {
		first := steady[0]
		last := steady[len(steady)-1]
		if last.TotalIdleWakeups >= first.TotalIdleWakeups {
			totalIdleDelta = last.TotalIdleWakeups - first.TotalIdleWakeups
		}
		if last.TotalInterruptWakeups >= first.TotalInterruptWakeups {
			totalInterruptDelta = last.TotalInterruptWakeups - first.TotalInterruptWakeups
		}
		out.IdleWakeupsPerSec = float64(totalIdleDelta) / wallSec
		out.InterruptWakeupsPerSec = float64(totalInterruptDelta) / wallSec
	}
	out.CPUSeconds = totalCPUSeconds
	if wallSec > 0 {
		out.CPUPercentMean = 100.0 * totalCPUSeconds / wallSec
	}
	return out
}

func summarizeRolePhase(role string, steady []GroupSample, wallSec float64) RolePhaseSummary {
	var samples []ProcessSample
	for _, gs := range steady {
		if ps, ok := gs.Roles[role]; ok {
			samples = append(samples, ps)
		}
	}
	rs := RolePhaseSummary{
		Role:        role,
		SampleCount: len(samples),
	}
	if len(samples) == 0 {
		return rs
	}
	rs.PID = samples[len(samples)-1].PID

	prim := make([]float64, 0, len(samples))
	commit := make([]float64, 0, len(samples))
	ws := make([]float64, 0, len(samples))
	rss := make([]float64, 0, len(samples))
	xs := make([]float64, 0, len(samples))
	var hasCommit, hasWS, hasRSS bool

	for i, ps := range samples {
		prim = append(prim, float64(ps.PrimaryMemoryBytes))
		xs = append(xs, float64(i))
		if ps.PrimaryMemoryBytes > rs.PeakPrimaryMemory {
			rs.PeakPrimaryMemory = ps.PrimaryMemoryBytes
		}
		if ps.LifetimeMaxPhysFootprintBytes > rs.PeakPrimaryMemory {
			rs.PeakPrimaryMemory = ps.LifetimeMaxPhysFootprintBytes
		}
		if ps.PrivateCommitBytes > 0 {
			hasCommit = true
		}
		commit = append(commit, float64(ps.PrivateCommitBytes))
		if ps.WorkingSetBytes > 0 {
			hasWS = true
		}
		ws = append(ws, float64(ps.WorkingSetBytes))
		if ps.RSSBytes > 0 {
			hasRSS = true
		}
		rss = append(rss, float64(ps.RSSBytes))

		if ps.Threads > rs.ThreadsMax {
			rs.ThreadsMax = ps.Threads
		}
		if ps.FDs > rs.FDsMax {
			rs.FDsMax = ps.FDs
		}
		if ps.Handles > rs.HandlesMax {
			rs.HandlesMax = ps.Handles
		}
		if ps.GDIObjects > rs.GDIObjectsMax {
			rs.GDIObjectsMax = ps.GDIObjects
		}
		if ps.USERObjects > rs.USERObjectsMax {
			rs.USERObjectsMax = ps.USERObjects
		}
		if ps.VisibleWindows > rs.VisibleWindowsMax {
			rs.VisibleWindowsMax = ps.VisibleWindows
		}
	}

	rs.PrimaryMemoryBytes = SummarizeValues(prim)
	if hasCommit {
		rs.PrivateCommitBytes = SummarizeValues(commit)
	}
	if hasWS {
		rs.WorkingSetBytes = SummarizeValues(ws)
	}
	if hasRSS {
		rs.RSSBytes = SummarizeValues(rss)
	}
	rs.LeakSlopeBytesPerSample, _ = LinearRegressionSlope(xs, prim)

	if len(samples) >= 2 {
		first := samples[0]
		last := samples[len(samples)-1]
		if last.PID == first.PID && last.CPUTotalNS >= first.CPUTotalNS {
			rs.CPUSeconds = float64(last.CPUTotalNS-first.CPUTotalNS) / 1e9
		}
		if wallSec > 0 {
			rs.CPUPercentMean = 100.0 * rs.CPUSeconds / wallSec
			if last.PID == first.PID && last.IdleWakeups >= first.IdleWakeups {
				rs.IdleWakeupsPerSec = float64(last.IdleWakeups-first.IdleWakeups) / wallSec
			}
			if last.PID == first.PID && last.InterruptWakeups >= first.InterruptWakeups {
				rs.InterruptWakeupsPerSec = float64(last.InterruptWakeups-first.InterruptWakeups) / wallSec
			}
		}
	}
	return rs
}

func wallDurationSec(samples []GroupSample) float64 {
	if len(samples) < 2 {
		return 0
	}
	first := samples[0]
	last := samples[len(samples)-1]
	if last.UTCMillis > first.UTCMillis {
		return float64(last.UTCMillis-first.UTCMillis) / 1000.0
	}
	if last.ElapsedSec > first.ElapsedSec {
		return last.ElapsedSec - first.ElapsedSec
	}
	return 0
}

func collectRoleNames(samples []GroupSample, expected []string) []string {
	seen := map[string]bool{}
	var out []string
	for _, r := range expected {
		if !seen[r] {
			seen[r] = true
			out = append(out, r)
		}
	}
	var extra []string
	for _, gs := range samples {
		for r := range gs.Roles {
			if !seen[r] {
				seen[r] = true
				extra = append(extra, r)
			}
		}
	}
	sort.Strings(extra)
	return append(out, extra...)
}

package report

import (
	"fmt"
	"sort"
	"strings"
)

// RenderMarkdown formats a Schema v1 Report as a concise GitHub-flavored Markdown report.
func RenderMarkdown(r *Report) string {
	if r == nil {
		return ""
	}
	var b strings.Builder
	fmt.Fprintf(&b, "# Desktop Resource Benchmark Report\n\n")
	fmt.Fprintf(&b, "- **Run ID**: `%s`\n", r.RunID)
	fmt.Fprintf(&b, "- **Generated (UTC)**: %s\n", r.GeneratedAtUTC)
	fmt.Fprintf(&b, "- **Track**: `%s` | **Preset**: `%s`\n", r.Track, r.Preset)
	fmt.Fprintf(&b, "- **Environment**: `%s/%s`", r.Environment.OS, r.Environment.HostArch)
	if r.Environment.OSVersion != "" {
		fmt.Fprintf(&b, " (OS `%s`)", r.Environment.OSVersion)
	}
	if r.Environment.VirtualMachine {
		fmt.Fprintf(&b, " [VM: %s]", nonEmpty(r.Environment.Hypervisor, "true"))
	}
	fmt.Fprintln(&b)
	if r.Environment.CPUModel != "" || r.Environment.LogicalCPUs > 0 {
		fmt.Fprintf(&b, "- **Hardware**: %s | %d logical CPUs | %s RAM\n",
			nonEmpty(r.Environment.CPUModel, "unknown CPU"),
			r.Environment.LogicalCPUs,
			formatBytes(r.Environment.PhysicalMemoryBytes))
	}
	if r.Environment.GitCommit != "" {
		fmt.Fprintf(&b, "- **Source**: `%s` (dirty=%t) | Go `%s`\n",
			r.Environment.GitCommit, r.Environment.GitDirty, r.Environment.GoVersion)
	}
	if r.ClipboardIntegrity.Available {
		fmt.Fprintf(&b, "- **Clipboard Integrity**: sequence %d -> %d (unchanged=%t)\n",
			r.ClipboardIntegrity.BeforeSequence, r.ClipboardIntegrity.AfterSequence, r.ClipboardIntegrity.Unchanged)
	}

	if len(r.Binaries) > 0 {
		fmt.Fprintf(&b, "\n## Binaries Under Test\n\n")
		fmt.Fprintf(&b, "| Binary | Host Arch | Binary Arch | Emulated | SHA-256 (prefix) |\n")
		fmt.Fprintf(&b, "|---|---|---|---|---|\n")
		names := make([]string, 0, len(r.Binaries))
		for k := range r.Binaries {
			names = append(names, k)
		}
		sort.Strings(names)
		for _, k := range names {
			bi := r.Binaries[k]
			sha := bi.SHA256
			if len(sha) > 12 {
				sha = sha[:12]
			}
			fmt.Fprintf(&b, "| `%s` | `%s` | `%s` | `%t` | `%s` |\n", k, bi.HostArch, bi.BinaryArch, bi.Emulated, sha)
		}
	}

	fmt.Fprintf(&b, "\n## Scenario Summary\n\n")
	fmt.Fprintf(&b, "| Scenario | Track | Dataset | Phase | Status | Primary Mem Median | Primary Mem P95 | Early Peak | CPU %% (1 core=100%%) | Wakeups/s | Leak Slope |\n")
	fmt.Fprintf(&b, "|---|---|---|---|---|---:|---:|---:|---:|---:|---:|\n")
	for _, sc := range r.Scenarios {
		ds := nonEmpty(sc.Dataset, sc.Preset)
		tr := nonEmpty(sc.Track, r.Track)
		if len(sc.Phases) == 0 {
			fmt.Fprintf(&b, "| `%s` | `%s` | `%s` | - | `%s` | - | - | - | - | - | - |\n", sc.ID, tr, ds, sc.Status)
			continue
		}
		for _, ph := range sc.Phases {
			slope := fmt.Sprintf("%.2f KiB/sample", ph.LeakSlopeBytesPerSample/1024.0)
			if ph.LeakSlopeBytesPerRound != 0 {
				slope = fmt.Sprintf("%.2f MiB/round", ph.LeakSlopeBytesPerRound/(1024.0*1024.0))
			}
			fmt.Fprintf(&b, "| `%s` | `%s` | `%s` | `%s` | `%s` | %s | %s | %s | %.2f%% | %.2f | %s |\n",
				sc.ID,
				tr,
				ds,
				ph.Phase,
				sc.Status,
				formatBytes(ph.SteadyMedianBytes),
				formatBytes(ph.SteadyP95Bytes),
				formatBytes(ph.EarlyPeakBytes),
				ph.CPUPercentMean,
				ph.IdleWakeupsPerSec,
				slope,
			)
		}
	}

	hasLatency := false
	for _, sc := range r.Scenarios {
		if len(sc.Latencies) > 0 {
			hasLatency = true
			break
		}
	}
	if hasLatency {
		fmt.Fprintf(&b, "\n## IPC-to-Ready Latencies\n\n")
		fmt.Fprintf(&b, "> Note: Latencies measure IPC command dispatch to observed readiness (`ipc_to_*_ms`) and exclude physical keyboard driver delivery and display compositor final scanout.\n\n")
		fmt.Fprintf(&b, "| Scenario | Metric | Count | P50 (ms) | P95 (ms) | P99 (ms) | Max (ms) |\n")
		fmt.Fprintf(&b, "|---|---|---:|---:|---:|---:|---:|\n")
		for _, sc := range r.Scenarios {
			keys := make([]string, 0, len(sc.Latencies))
			for k := range sc.Latencies {
				keys = append(keys, k)
			}
			sort.Strings(keys)
			for _, k := range keys {
				lat := sc.Latencies[k]
				fmt.Fprintf(&b, "| `%s` | `%s` | %d | %.1f | %.1f | %.1f | %.1f |\n",
					sc.ID, k, lat.Count, lat.P50Ms, lat.P95Ms, lat.P99Ms, lat.MaxMs)
			}
		}
	}

	if len(r.DerivedMetrics) > 0 {
		fmt.Fprintf(&b, "\n## Derived Metrics\n\n")
		keys := make([]string, 0, len(r.DerivedMetrics))
		for k := range r.DerivedMetrics {
			keys = append(keys, k)
		}
		sort.Strings(keys)
		for _, k := range keys {
			fmt.Fprintf(&b, "- `%s`: `%.2f`\n", k, r.DerivedMetrics[k])
		}
	}

	return b.String()
}

func formatBytes(v uint64) string {
	const (
		kib = 1024.0
		mib = 1024.0 * 1024.0
		gib = 1024.0 * 1024.0 * 1024.0
	)
	f := float64(v)
	switch {
	case f >= gib:
		return fmt.Sprintf("%.2f GiB", f/gib)
	case f >= mib:
		return fmt.Sprintf("%.2f MiB", f/mib)
	case f >= kib:
		return fmt.Sprintf("%.1f KiB", f/kib)
	default:
		return fmt.Sprintf("%d B", v)
	}
}

func nonEmpty(v, fallback string) string {
	if strings.TrimSpace(v) == "" {
		return fallback
	}
	return v
}

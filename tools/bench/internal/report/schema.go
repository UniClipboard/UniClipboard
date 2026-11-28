// Package report defines the Schema v1 benchmark report format, Markdown
// rendering, SHA256SUMS generation, and regression/budget comparison.
package report

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/uniclipboard/unic/tools/bench/internal/sampler"
)

// SchemaVersion is the current machine-readable benchmark report schema version.
const SchemaVersion = 1

// Report is the top-level Schema v1 benchmark artifact written to report.json.
type Report struct {
	SchemaVersion      int                   `json:"schema_version"`
	RunID              string                `json:"run_id"`
	GeneratedAtUTC     string                `json:"generated_at_utc"`
	Track              string                `json:"track"`  // "production" (Track A) or "interactive" (Track B)
	Preset             string                `json:"preset"` // "empty", "standard", "soak"
	Environment        EnvironmentInfo       `json:"environment"`
	Binaries           map[string]BinaryInfo `json:"binaries,omitempty"`
	Fixture            FixtureInfo           `json:"fixture"`
	ClipboardIntegrity ClipboardCheck        `json:"clipboard_integrity"`
	Scenarios          []ScenarioResult      `json:"scenarios"`
	DerivedMetrics     map[string]float64    `json:"derived_metrics,omitempty"`
}

// EnvironmentInfo records host hardware, OS, VM status, and source identity.
type EnvironmentInfo struct {
	OS                  string `json:"os"`
	HostArch            string `json:"host_arch"`
	OSVersion           string `json:"os_version,omitempty"`
	CPUModel            string `json:"cpu_model,omitempty"`
	LogicalCPUs         int    `json:"logical_cpus"`
	PhysicalMemoryBytes uint64 `json:"physical_memory_bytes,omitempty"`
	VirtualMachine      bool   `json:"virtual_machine,omitempty"`
	Hypervisor          string `json:"hypervisor,omitempty"`
	GoVersion           string `json:"go_version,omitempty"`
	GitCommit           string `json:"git_commit,omitempty"`
	GitDirty            bool   `json:"git_dirty,omitempty"`
}

// BinaryInfo records build provenance and authoritative architecture for a binary under test.
type BinaryInfo struct {
	Path       string   `json:"path"`
	SHA256     string   `json:"sha256"`
	HostArch   string   `json:"host_arch"`
	BinaryArch string   `json:"binary_arch"`
	Emulated   bool     `json:"emulated"`
	BuildTags  []string `json:"build_tags,omitempty"`
}

// FixtureInfo records the deterministic corpus used during the run.
type FixtureInfo struct {
	Preset         string `json:"preset"`
	Seed           uint64 `json:"seed"`
	ManifestSHA256 string `json:"manifest_sha256,omitempty"`
	TextCount      int    `json:"text_count"`
	ImageCount     int    `json:"image_count"`
	FileCount      int    `json:"file_count"`
}

// ClipboardCheck verifies the host/guest clipboard sequence did not mutate during the benchmark.
type ClipboardCheck struct {
	Available      bool   `json:"available"`
	BeforeSequence uint64 `json:"before_sequence"`
	AfterSequence  uint64 `json:"after_sequence"`
	Unchanged      bool   `json:"unchanged"`
	Note           string `json:"note,omitempty"`
}

// LatencyMetric captures IPC-to-readiness latency distributions (explicitly excluding
// physical hotkey driver dispatch and display compositor final scanout).
type LatencyMetric struct {
	Name      string    `json:"name"`
	Count     int       `json:"count"`
	P50Ms     float64   `json:"p50_ms"`
	P95Ms     float64   `json:"p95_ms"`
	P99Ms     float64   `json:"p99_ms"`
	MaxMs     float64   `json:"max_ms"`
	SamplesMs []float64 `json:"samples_ms,omitempty"`
	Note      string    `json:"note,omitempty"`
}

// LeakSummary records multi-round linear regression slope over post-cycle memory.
type LeakSummary struct {
	Metric             string   `json:"metric"`
	Rounds             int      `json:"rounds"`
	SlopeBytesPerRound float64  `json:"slope_bytes_per_round"`
	ValuesBytes        []uint64 `json:"values_bytes,omitempty"`
	Passed             bool     `json:"passed"`
}

// ProbeRoleSummary holds aggregated Go runtime and optional desktopui cache metrics from bench_probe.
type ProbeRoleSummary struct {
	Role                  string `json:"role"`
	SampleCount           int    `json:"sample_count"`
	HeapAllocMaxBytes     uint64 `json:"heap_alloc_max_bytes"`
	HeapInuseLastBytes    uint64 `json:"heap_inuse_last_bytes"`
	HeapSysLastBytes      uint64 `json:"heap_sys_last_bytes"`
	HeapReleasedLastBytes uint64 `json:"heap_released_last_bytes"`
	StackInuseMaxBytes    uint64 `json:"stack_inuse_max_bytes"`
	NumGCDelta            uint64 `json:"num_gc_delta"`
	GoroutinesMax         uint64 `json:"goroutines_max"`
	EntriesMax            int    `json:"entries_max,omitempty"`
	DetailsMax            int    `json:"details_max,omitempty"`
	ImageOwnersMax        int    `json:"image_owners_max,omitempty"`
	SelectedOriginalsMax  int    `json:"selected_originals_max,omitempty"`
}

// ScenarioResult holds summarized phases, latencies, attribution, and pass/fail state for one scenario.
type ScenarioResult struct {
	ID             string                        `json:"id"`
	Dataset        string                        `json:"dataset,omitempty"`
	Track          string                        `json:"track"`
	Preset         string                        `json:"preset"`
	Rounds         int                           `json:"rounds"`
	Status         string                        `json:"status"` // PASS, FAIL, SKIP
	Error          string                        `json:"error,omitempty"`
	ExpectedRoles  []string                      `json:"expected_roles,omitempty"`
	Phases         []sampler.PhaseSummary        `json:"phases,omitempty"`
	Attribution    []sampler.AttributionSnapshot `json:"attribution,omitempty"`
	Latencies      map[string]LatencyMetric      `json:"latencies,omitempty"`
	LeakCheck      *LeakSummary                  `json:"leak_check,omitempty"`
	DerivedMetrics map[string]float64            `json:"derived_metrics,omitempty"`
	ProbeByRole    map[string]ProbeRoleSummary   `json:"probe_by_role,omitempty"`
	Notes          []string                      `json:"notes,omitempty"`
}

// WriteJSON serializes r as indented Schema v1 JSON to path.
func WriteJSON(path string, r *Report) error {
	if r == nil {
		return errors.New("nil report")
	}
	if r.SchemaVersion == 0 {
		r.SchemaVersion = SchemaVersion
	}
	if r.SchemaVersion != SchemaVersion {
		return fmt.Errorf("unsupported schema version %d (want %d)", r.SchemaVersion, SchemaVersion)
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return fmt.Errorf("create report dir: %w", err)
	}
	raw, err := json.MarshalIndent(r, "", "  ")
	if err != nil {
		return fmt.Errorf("marshal report json: %w", err)
	}
	raw = append(raw, '\n')
	if err := os.WriteFile(path, raw, 0o600); err != nil {
		return fmt.Errorf("write report json: %w", err)
	}
	return nil
}

// ReadJSON loads and validates a Schema v1 Report from path.
func ReadJSON(path string) (*Report, error) {
	raw, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("read report %s: %w", path, err)
	}
	var r Report
	if err := json.Unmarshal(raw, &r); err != nil {
		return nil, fmt.Errorf("parse report %s: %w", path, err)
	}
	if r.SchemaVersion != SchemaVersion {
		return nil, fmt.Errorf("report %s has schema_version=%d, want %d", path, r.SchemaVersion, SchemaVersion)
	}
	return &r, nil
}

// WriteSHA256SUMS writes `<sha256>  <filename>` entries for files inside dir to dir/SHA256SUMS.
func WriteSHA256SUMS(dir string, filenames ...string) (string, error) {
	if len(filenames) == 0 {
		return "", errors.New("at least one filename is required for SHA256SUMS")
	}
	names := append([]string(nil), filenames...)
	sort.Strings(names)

	var b strings.Builder
	for _, name := range names {
		clean := filepath.Base(name)
		data, err := os.ReadFile(filepath.Join(dir, clean))
		if err != nil {
			return "", fmt.Errorf("hash %s: %w", clean, err)
		}
		sum := sha256.Sum256(data)
		fmt.Fprintf(&b, "%s  %s\n", hex.EncodeToString(sum[:]), clean)
	}
	outPath := filepath.Join(dir, "SHA256SUMS")
	if err := os.WriteFile(outPath, []byte(b.String()), 0o600); err != nil {
		return "", fmt.Errorf("write SHA256SUMS: %w", err)
	}
	return outPath, nil
}

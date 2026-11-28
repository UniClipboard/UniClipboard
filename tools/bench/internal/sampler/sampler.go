// Package sampler collects OS-native per-process resource metrics across macOS
// and Windows, performs post-phase memory attribution, and summarizes steady-state
// distributions and leak regression slopes.
package sampler

import (
	"context"
	"errors"
	"fmt"
	"runtime"
	"time"
)

// ErrUnsupportedPlatform is returned when native sampling is invoked on an OS
// other than macOS or Windows.
var ErrUnsupportedPlatform = errors.New("native resource sampling is only supported on darwin and windows")

// ProcessSample holds one point-in-time native OS measurement of a single PID.
type ProcessSample struct {
	SampleIndex int     `json:"sample_index"`
	Phase       string  `json:"phase"`
	ElapsedSec  float64 `json:"elapsed_sec"`
	UTCMillis   int64   `json:"utc_millis"`
	Role        string  `json:"role"`
	PID         int     `json:"pid"`
	Alive       bool    `json:"alive"`

	// PrimaryMemoryBytes is the cross-platform headline physical memory metric:
	// macOS: ri_phys_footprint (matches Activity Monitor Memory)
	// Windows: private_working_set_bytes (matches Task Manager Memory)
	PrimaryMemoryBytes uint64 `json:"primary_memory_bytes"`

	// macOS memory fields (bytes)
	PhysFootprintBytes            uint64 `json:"phys_footprint_bytes,omitempty"`
	LifetimeMaxPhysFootprintBytes uint64 `json:"lifetime_max_phys_footprint_bytes,omitempty"`
	RSSBytes                      uint64 `json:"rss_bytes,omitempty"`

	// Windows memory fields (bytes)
	PrivateWorkingSetBytes uint64 `json:"private_working_set_bytes,omitempty"`
	PrivateCommitBytes     uint64 `json:"private_commit_bytes,omitempty"`
	WorkingSetBytes        uint64 `json:"working_set_bytes,omitempty"`
	PeakWorkingSetBytes    uint64 `json:"peak_working_set_bytes,omitempty"`

	// CPU & power proxy
	CPUTotalNS       uint64 `json:"cpu_total_ns"`
	CPUUserNS        uint64 `json:"cpu_user_ns"`
	CPUSystemNS      uint64 `json:"cpu_system_ns"`
	IdleWakeups      uint64 `json:"idle_wakeups,omitempty"`
	InterruptWakeups uint64 `json:"interrupt_wakeups,omitempty"`

	// OS handles, threads, and native window state
	Threads           int    `json:"threads"`
	ContextSwitches   uint64 `json:"context_switches,omitempty"`
	FDs               int    `json:"fds,omitempty"`
	Handles           uint32 `json:"handles,omitempty"`
	GDIObjects        uint32 `json:"gdi_objects,omitempty"`
	USERObjects       uint32 `json:"user_objects,omitempty"`
	VisibleWindows    int    `json:"visible_windows,omitempty"`
	ForegroundWindows int    `json:"foreground_windows,omitempty"`
}

// GroupSample aggregates one synchronized tick across all expected role PIDs.
type GroupSample struct {
	SampleIndex int                      `json:"sample_index"`
	Phase       string                   `json:"phase"`
	ElapsedSec  float64                  `json:"elapsed_sec"`
	UTCMillis   int64                    `json:"utc_millis"`
	Roles       map[string]ProcessSample `json:"roles"`

	TotalPrimaryMemoryBytes uint64   `json:"total_primary_memory_bytes"`
	TotalPrivateCommitBytes uint64   `json:"total_private_commit_bytes,omitempty"`
	TotalWorkingSetBytes    uint64   `json:"total_working_set_bytes,omitempty"`
	TotalCPUTotalNS         uint64   `json:"total_cpu_total_ns"`
	TotalIdleWakeups        uint64   `json:"total_idle_wakeups,omitempty"`
	TotalInterruptWakeups   uint64   `json:"total_interrupt_wakeups,omitempty"`
	TotalThreads            int      `json:"total_threads"`
	TotalFDs                int      `json:"total_fds,omitempty"`
	TotalHandles            uint32   `json:"total_handles,omitempty"`
	TotalGDIObjects         uint32   `json:"total_gdi_objects,omitempty"`
	TotalUSERObjects        uint32   `json:"total_user_objects,omitempty"`
	MissingRoles            []string `json:"missing_roles,omitempty"`
}

// MetricStats holds summary distribution statistics for a numeric series.
type MetricStats struct {
	Count  int     `json:"count"`
	Min    float64 `json:"min"`
	Max    float64 `json:"max"`
	Mean   float64 `json:"mean"`
	Median float64 `json:"median"`
	P95    float64 `json:"p95"`
	P99    float64 `json:"p99"`
}

// RolePhaseSummary summarizes a single process role within a phase.
type RolePhaseSummary struct {
	Role                    string      `json:"role"`
	PID                     int         `json:"pid"`
	SampleCount             int         `json:"sample_count"`
	PrimaryMemoryBytes      MetricStats `json:"primary_memory_bytes"`
	PeakPrimaryMemory       uint64      `json:"peak_primary_memory_bytes"`
	PrivateCommitBytes      MetricStats `json:"private_commit_bytes,omitempty"`
	WorkingSetBytes         MetricStats `json:"working_set_bytes,omitempty"`
	RSSBytes                MetricStats `json:"rss_bytes,omitempty"`
	CPUSeconds              float64     `json:"cpu_seconds"`
	CPUPercentMean          float64     `json:"cpu_percent_mean"` // 100% = 1 logical core
	IdleWakeupsPerSec       float64     `json:"idle_wakeups_per_sec,omitempty"`
	InterruptWakeupsPerSec  float64     `json:"interrupt_wakeups_per_sec,omitempty"`
	ThreadsMax              int         `json:"threads_max"`
	FDsMax                  int         `json:"fds_max,omitempty"`
	HandlesMax              uint32      `json:"handles_max,omitempty"`
	GDIObjectsMax           uint32      `json:"gdi_objects_max,omitempty"`
	USERObjectsMax          uint32      `json:"user_objects_max,omitempty"`
	VisibleWindowsMax       int         `json:"visible_windows_max,omitempty"`
	LeakSlopeBytesPerSample float64     `json:"leak_slope_bytes_per_sample"`
}

// PhaseSummary summarizes a complete benchmark phase across a process group.
type PhaseSummary struct {
	Phase                   string                      `json:"phase"`
	DurationSec             float64                     `json:"duration_sec"`
	WarmupTrimmedSamples    int                         `json:"warmup_trimmed_samples"`
	SampleCount             int                         `json:"sample_count"`
	ExpectedRoles           []string                    `json:"expected_roles"`
	MissingRoles            []string                    `json:"missing_roles,omitempty"`
	PrimaryMemoryBytes      MetricStats                 `json:"primary_memory_bytes"`
	PeakPrimaryMemoryBytes  uint64                      `json:"peak_primary_memory_bytes"`
	EarlyPeakBytes          uint64                      `json:"early_peak_bytes"`
	SteadyMedianBytes       uint64                      `json:"steady_median_bytes"`
	SteadyP95Bytes          uint64                      `json:"steady_p95_bytes"`
	PrivateCommitBytes      MetricStats                 `json:"private_commit_bytes,omitempty"`
	WorkingSetBytes         MetricStats                 `json:"working_set_bytes,omitempty"`
	WorkingSetOverlapNote   string                      `json:"working_set_overlap_note,omitempty"`
	CPUSeconds              float64                     `json:"cpu_seconds"`
	CPUPercentMean          float64                     `json:"cpu_percent_mean"`
	IdleWakeupsPerSec       float64                     `json:"idle_wakeups_per_sec,omitempty"`
	InterruptWakeupsPerSec  float64                     `json:"interrupt_wakeups_per_sec,omitempty"`
	LeakSlopeBytesPerSample float64                     `json:"leak_slope_bytes_per_sample"`
	LeakSlopeBytesPerRound  float64                     `json:"leak_slope_bytes_per_round,omitempty"`
	ByRole                  map[string]RolePhaseSummary `json:"by_role"`
}

// FootprintCategory holds one category from macOS `footprint -j`.
type FootprintCategory struct {
	DirtyBytes       uint64 `json:"dirty_bytes"`
	SwappedBytes     uint64 `json:"swapped_bytes"`
	CleanBytes       uint64 `json:"clean_bytes"`
	ReclaimableBytes uint64 `json:"reclaimable_bytes"`
	WiredBytes       uint64 `json:"wired_bytes"`
	Regions          int    `json:"regions"`
}

// ProcessAttribution captures single-shot post-phase native memory classification.
type ProcessAttribution struct {
	PID  int    `json:"pid"`
	Role string `json:"role"`

	// macOS `footprint -j` + `top -l 1 -stats pid,ports`
	PhysFootprintBytes     uint64                       `json:"phys_footprint_bytes,omitempty"`
	PhysFootprintPeakBytes uint64                       `json:"phys_footprint_peak_bytes,omitempty"`
	IOSurfaceBytes         uint64                       `json:"iosurface_bytes,omitempty"`
	IOAcceleratorBytes     uint64                       `json:"ioaccelerator_bytes,omitempty"`
	UnmappedOwnedBytes     uint64                       `json:"unmapped_owned_bytes,omitempty"`
	MallocBytes            uint64                       `json:"malloc_bytes,omitempty"`
	VMAllocateBytes        uint64                       `json:"vm_allocate_bytes,omitempty"`
	MachPorts              int                          `json:"mach_ports,omitempty"`
	Categories             map[string]FootprintCategory `json:"categories,omitempty"`

	// Windows `VirtualQueryEx` region commits (explicitly not GPU VRAM)
	PrivateCommittedBytes uint64 `json:"private_committed_bytes,omitempty"`
	MappedCommittedBytes  uint64 `json:"mapped_committed_bytes,omitempty"`
	ImageCommittedBytes   uint64 `json:"image_committed_bytes,omitempty"`
	ReservedBytes         uint64 `json:"reserved_bytes,omitempty"`
	Note                  string `json:"note,omitempty"`
}

// AttributionSnapshot holds single-shot end-of-phase attribution for a process group.
type AttributionSnapshot struct {
	Phase         string                     `json:"phase"`
	Platform      string                     `json:"platform"`
	CapturedAtUTC string                     `json:"captured_at_utc"`
	Processes     map[int]ProcessAttribution `json:"processes"`
	Warnings      []string                   `json:"warnings,omitempty"`
}

// HostInfo describes the local OS and hardware environment.
type HostInfo struct {
	OS                  string `json:"os"`
	HostArch            string `json:"host_arch"`
	OSVersion           string `json:"os_version"`
	CPUModel            string `json:"cpu_model"`
	LogicalCPUs         int    `json:"logical_cpus"`
	PhysicalMemoryBytes uint64 `json:"physical_memory_bytes"`
	VirtualMachine      bool   `json:"virtual_machine,omitempty"`
	Hypervisor          string `json:"hypervisor,omitempty"`
}

// SampleGroup captures one synchronized GroupSample across rolePIDs and flags
// any missing or dead expected role.
func SampleGroup(sampleIndex int, phase string, elapsedSec float64, rolePIDs map[string]int, expectedRoles []string) (GroupSample, error) {
	nowMs := time.Now().UTC().UnixMilli()
	gs := GroupSample{
		SampleIndex: sampleIndex,
		Phase:       phase,
		ElapsedSec:  elapsedSec,
		UTCMillis:   nowMs,
		Roles:       make(map[string]ProcessSample, len(rolePIDs)),
	}

	missingSet := make(map[string]bool)
	for _, r := range expectedRoles {
		if pid, ok := rolePIDs[r]; !ok || pid <= 0 {
			missingSet[r] = true
		}
	}

	for role, pid := range rolePIDs {
		if pid <= 0 {
			missingSet[role] = true
			continue
		}
		ps, err := SampleProcess(pid)
		if err != nil {
			missingSet[role] = true
			continue
		}
		ps.SampleIndex = sampleIndex
		ps.Phase = phase
		ps.ElapsedSec = elapsedSec
		ps.UTCMillis = nowMs
		ps.Role = role
		gs.Roles[role] = ps

		gs.TotalPrimaryMemoryBytes += ps.PrimaryMemoryBytes
		gs.TotalPrivateCommitBytes += ps.PrivateCommitBytes
		gs.TotalWorkingSetBytes += ps.WorkingSetBytes
		gs.TotalCPUTotalNS += ps.CPUTotalNS
		gs.TotalIdleWakeups += ps.IdleWakeups
		gs.TotalInterruptWakeups += ps.InterruptWakeups
		gs.TotalThreads += ps.Threads
		gs.TotalFDs += ps.FDs
		gs.TotalHandles += ps.Handles
		gs.TotalGDIObjects += ps.GDIObjects
		gs.TotalUSERObjects += ps.USERObjects
	}

	for _, r := range expectedRoles {
		if missingSet[r] {
			gs.MissingRoles = append(gs.MissingRoles, r)
		}
	}
	if len(gs.MissingRoles) > 0 {
		return gs, fmt.Errorf("missing or exited expected roles: %v", gs.MissingRoles)
	}
	return gs, nil
}

// SampleProcess collects native OS resource metrics for pid.
func SampleProcess(pid int) (ProcessSample, error) {
	return sampleProcessNative(pid)
}

// CaptureAttribution runs single-shot end-of-phase memory classification.
func CaptureAttribution(ctx context.Context, phase string, rolePIDs map[string]int) (*AttributionSnapshot, error) {
	return captureAttributionNative(ctx, phase, rolePIDs)
}

// ClipboardSequence returns the current OS clipboard change/sequence counter
// without modifying clipboard contents.
func ClipboardSequence() (uint64, error) {
	return clipboardSequenceNative()
}

// HostArch returns the true host CPU architecture (even when running under
// emulation such as amd64 on Windows ARM64).
func HostArch() string {
	return hostArchNative()
}

// CollectHostInfo probes OS version, CPU model, logical CPU count, and RAM.
func CollectHostInfo(ctx context.Context) HostInfo {
	info := HostInfo{
		OS:          runtime.GOOS,
		HostArch:    HostArch(),
		LogicalCPUs: runtime.NumCPU(),
	}
	fillHostInfoNative(ctx, &info)
	return info
}

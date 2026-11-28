//go:build darwin

package sampler

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"time"
)

// FootprintProcess holds one process's parsed `/usr/bin/footprint -j` output.
type FootprintProcess struct {
	Name       string                     `json:"name"`
	PID        int                        `json:"pid"`
	Translated bool                       `json:"translated"`
	PageSize   uint64                     `json:"page size"`
	Footprint  uint64                     `json:"footprint"`
	Categories map[string]footprintCatRaw `json:"categories"`
	Auxiliary  footprintAuxiliary         `json:"auxiliary"`
}

type footprintCatRaw struct {
	Dirty       uint64 `json:"dirty"`
	Swapped     uint64 `json:"swapped"`
	Clean       uint64 `json:"clean"`
	Reclaimable uint64 `json:"reclaimable"`
	Wired       uint64 `json:"wired"`
	Regions     int    `json:"regions"`
}

type footprintAuxiliary struct {
	PhysFootprint     uint64 `json:"phys_footprint"`
	PhysFootprintPeak uint64 `json:"phys_footprint_peak"`
}

// FootprintReport represents the JSON payload emitted by `/usr/bin/footprint -j`.
type FootprintReport struct {
	Unit           string             `json:"unit"`
	Processes      []FootprintProcess `json:"processes"`
	Errors         []any              `json:"errors"`
	Warnings       []any              `json:"warnings"`
	TotalFootprint uint64             `json:"total footprint"`
	PageSize       uint64             `json:"page size"`
}

// ParseFootprintJSON parses raw JSON produced by `/usr/bin/footprint -j`.
func ParseFootprintJSON(data []byte) (*FootprintReport, error) {
	var rep FootprintReport
	if err := json.Unmarshal(data, &rep); err != nil {
		return nil, fmt.Errorf("parse footprint json: %w", err)
	}
	return &rep, nil
}

// ParseTopPorts parses `top -l 1 -stats pid,ports` output into a pid -> port count map.
func ParseTopPorts(output string) map[int]int {
	ports := make(map[int]int)
	inTable := false
	for _, rawLine := range strings.Split(output, "\n") {
		line := strings.TrimSpace(rawLine)
		if line == "" {
			continue
		}
		fields := strings.Fields(line)
		if len(fields) >= 2 && strings.EqualFold(fields[0], "PID") && strings.Contains(strings.ToUpper(fields[1]), "PORT") {
			inTable = true
			continue
		}
		if !inTable || len(fields) < 2 {
			continue
		}
		pid, err := strconv.Atoi(strings.TrimSuffix(fields[0], "*"))
		if err != nil || pid <= 0 {
			continue
		}
		portStr := strings.TrimRight(fields[1], "+-")
		portCount, err := strconv.Atoi(portStr)
		if err != nil || portCount < 0 {
			continue
		}
		ports[pid] = portCount
	}
	return ports
}

// CaptureFootprint executes `/usr/bin/footprint -j` once for the given PIDs.
func CaptureFootprint(ctx context.Context, pids []int) (*FootprintReport, error) {
	if len(pids) == 0 {
		return &FootprintReport{}, nil
	}
	tmpDir, err := os.MkdirTemp("", "unic-footprint-")
	if err != nil {
		return nil, fmt.Errorf("create temp dir for footprint: %w", err)
	}
	defer func() { _ = os.RemoveAll(tmpDir) }()

	outPath := filepath.Join(tmpDir, "footprint.json")
	args := make([]string, 0, 2+len(pids)*2)
	args = append(args, "-j", outPath)
	for _, pid := range pids {
		if pid > 0 {
			args = append(args, "-p", strconv.Itoa(pid))
		}
	}
	cmd := exec.CommandContext(ctx, "/usr/bin/footprint", args...)
	if out, err := cmd.CombinedOutput(); err != nil {
		return nil, fmt.Errorf("run footprint -j: %w: %s", err, strings.TrimSpace(string(out)))
	}
	data, err := os.ReadFile(outPath)
	if err != nil {
		return nil, fmt.Errorf("read footprint json: %w", err)
	}
	return ParseFootprintJSON(data)
}

// CaptureMachPorts runs `top -l 1 -stats pid,ports` once to read Mach port counts.
func CaptureMachPorts(ctx context.Context, pids []int) (map[int]int, error) {
	if len(pids) == 0 {
		return map[int]int{}, nil
	}
	args := make([]string, 0, 5+len(pids)*2)
	args = append(args, "-l", "1", "-stats", "pid,ports")
	for _, pid := range pids {
		if pid > 0 {
			args = append(args, "-pid", strconv.Itoa(pid))
		}
	}
	cmd := exec.CommandContext(ctx, "/usr/bin/top", args...)
	out, err := cmd.CombinedOutput()
	if err != nil {
		return nil, fmt.Errorf("run top for ports: %w: %s", err, strings.TrimSpace(string(out)))
	}
	return ParseTopPorts(string(out)), nil
}

func captureAttributionNative(ctx context.Context, phase string, rolePIDs map[string]int) (*AttributionSnapshot, error) {
	snap := &AttributionSnapshot{
		Phase:         phase,
		Platform:      "darwin",
		CapturedAtUTC: time.Now().UTC().Format(time.RFC3339Nano),
		Processes:     make(map[int]ProcessAttribution, len(rolePIDs)),
	}

	pidToRole := make(map[int]string, len(rolePIDs))
	pids := make([]int, 0, len(rolePIDs))
	for role, pid := range rolePIDs {
		if pid <= 0 {
			continue
		}
		pidToRole[pid] = role
		pids = append(pids, pid)
	}
	if len(pids) == 0 {
		return snap, nil
	}

	fpRep, err := CaptureFootprint(ctx, pids)
	if err != nil {
		snap.Warnings = append(snap.Warnings, err.Error())
	} else {
		for _, proc := range fpRep.Processes {
			pa := ProcessAttribution{
				PID:                    proc.PID,
				Role:                   pidToRole[proc.PID],
				PhysFootprintBytes:     proc.Auxiliary.PhysFootprint,
				PhysFootprintPeakBytes: proc.Auxiliary.PhysFootprintPeak,
				Categories:             make(map[string]FootprintCategory, len(proc.Categories)),
			}
			if pa.PhysFootprintBytes == 0 {
				pa.PhysFootprintBytes = proc.Footprint
			}
			for catName, raw := range proc.Categories {
				pa.Categories[catName] = FootprintCategory{
					DirtyBytes:       raw.Dirty,
					SwappedBytes:     raw.Swapped,
					CleanBytes:       raw.Clean,
					ReclaimableBytes: raw.Reclaimable,
					WiredBytes:       raw.Wired,
					Regions:          raw.Regions,
				}
				footBytes := raw.Dirty + raw.Swapped
				lower := strings.ToLower(catName)
				switch {
				case strings.Contains(lower, "iosurface"):
					pa.IOSurfaceBytes += footBytes
				case strings.Contains(lower, "ioaccelerator"):
					pa.IOAcceleratorBytes += footBytes
				case strings.Contains(lower, "unmapped"):
					pa.UnmappedOwnedBytes += footBytes
				case strings.HasPrefix(lower, "malloc"):
					pa.MallocBytes += footBytes
				case catName == "VM_ALLOCATE":
					pa.VMAllocateBytes += footBytes
				}
			}
			snap.Processes[proc.PID] = pa
		}
	}

	if ports, err := CaptureMachPorts(ctx, pids); err != nil {
		snap.Warnings = append(snap.Warnings, err.Error())
	} else {
		for pid, pcount := range ports {
			pa, ok := snap.Processes[pid]
			if !ok {
				pa = ProcessAttribution{PID: pid, Role: pidToRole[pid]}
			}
			pa.MachPorts = pcount
			snap.Processes[pid] = pa
		}
	}

	return snap, nil
}

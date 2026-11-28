package scenarios

import (
	"bufio"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"

	"github.com/uniclipboard/unic/tools/bench/internal/report"
)

type probeRuntimeRow struct {
	UTCMillis    int64   `json:"utc_millis"`
	ElapsedSec   float64 `json:"elapsed_sec"`
	Role         string  `json:"role"`
	PID          int     `json:"pid"`
	HeapAlloc    uint64  `json:"heap_alloc"`
	HeapInuse    uint64  `json:"heap_inuse"`
	HeapSys      uint64  `json:"heap_sys"`
	HeapReleased uint64  `json:"heap_released"`
	StackInuse   uint64  `json:"stack_inuse"`
	NumGC        uint64  `json:"num_gc"`
	Goroutines   uint64  `json:"goroutines"`
	Cache        *struct {
		Entries           int `json:"entries"`
		MetadataItems     int `json:"metadata_items"`
		Details           int `json:"details"`
		ImageOwners       int `json:"image_owners"`
		SelectedOriginals int `json:"selected_originals"`
	} `json:"cache,omitempty"`
}

type probeEventRow struct {
	Event        string `json:"event"`
	Role         string `json:"role"`
	PID          int    `json:"pid"`
	Seq          uint64 `json:"seq"`
	SelectionSeq uint64 `json:"selection_seq"`
	UnixMillis   int64  `json:"unix_millis"`
	Width        int    `json:"width"`
	Height       int    `json:"height"`
}

// ParseRuntimeMetricsDir aggregates all *.runtime.jsonl files written by bench_probe.
func ParseRuntimeMetricsDir(dir string) (map[string]report.ProbeRoleSummary, error) {
	if dir == "" {
		return nil, nil
	}
	entries, err := os.ReadDir(dir)
	if err != nil {
		if os.IsNotExist(err) {
			return nil, nil
		}
		return nil, err
	}

	byRole := map[string][]probeRuntimeRow{}
	for _, de := range entries {
		if de.IsDir() || !strings.HasSuffix(de.Name(), ".runtime.jsonl") {
			continue
		}
		f, err := os.Open(filepath.Join(dir, de.Name()))
		if err != nil {
			return nil, err
		}
		sc := bufio.NewScanner(f)
		for sc.Scan() {
			line := strings.TrimSpace(sc.Text())
			if line == "" {
				continue
			}
			var row probeRuntimeRow
			if uerr := json.Unmarshal([]byte(line), &row); uerr == nil && row.Role != "" {
				byRole[row.Role] = append(byRole[row.Role], row)
			}
		}
		_ = f.Close()
	}

	out := make(map[string]report.ProbeRoleSummary, len(byRole))
	for role, rows := range byRole {
		if len(rows) == 0 {
			continue
		}
		firstGC := rows[0].NumGC
		last := rows[len(rows)-1]
		s := report.ProbeRoleSummary{
			Role:                  role,
			SampleCount:           len(rows),
			HeapInuseLastBytes:    last.HeapInuse,
			HeapSysLastBytes:      last.HeapSys,
			HeapReleasedLastBytes: last.HeapReleased,
		}
		if last.NumGC >= firstGC {
			s.NumGCDelta = last.NumGC - firstGC
		}
		for _, r := range rows {
			if r.HeapAlloc > s.HeapAllocMaxBytes {
				s.HeapAllocMaxBytes = r.HeapAlloc
			}
			if r.StackInuse > s.StackInuseMaxBytes {
				s.StackInuseMaxBytes = r.StackInuse
			}
			if r.Goroutines > s.GoroutinesMax {
				s.GoroutinesMax = r.Goroutines
			}
			if r.Cache != nil {
				if r.Cache.Entries > s.EntriesMax {
					s.EntriesMax = r.Cache.Entries
				}
				if r.Cache.Details > s.DetailsMax {
					s.DetailsMax = r.Cache.Details
				}
				if r.Cache.ImageOwners > s.ImageOwnersMax {
					s.ImageOwnersMax = r.Cache.ImageOwners
				}
				if r.Cache.SelectedOriginals > s.SelectedOriginalsMax {
					s.SelectedOriginalsMax = r.Cache.SelectedOriginals
				}
			}
		}
		out[role] = s
	}
	return out, nil
}

// ReadOriginalReadyEvents parses *.events.jsonl for selected-original-ready events >= sinceMs.
func ReadOriginalReadyEvents(dir, role string, pid int, sinceMs int64) ([]probeEventRow, error) {
	if dir == "" {
		return nil, nil
	}
	entries, err := os.ReadDir(dir)
	if err != nil {
		if os.IsNotExist(err) {
			return nil, nil
		}
		return nil, err
	}
	var events []probeEventRow
	for _, de := range entries {
		if de.IsDir() || !strings.HasSuffix(de.Name(), ".events.jsonl") {
			continue
		}
		f, err := os.Open(filepath.Join(dir, de.Name()))
		if err != nil {
			return nil, err
		}
		sc := bufio.NewScanner(f)
		for sc.Scan() {
			var ev probeEventRow
			if json.Unmarshal([]byte(sc.Text()), &ev) == nil && ev.Event == "selected-original-ready" {
				if (role == "" || ev.Role == role) && (pid <= 0 || ev.PID == pid) && ev.UnixMillis >= sinceMs {
					events = append(events, ev)
				}
			}
		}
		_ = f.Close()
	}
	return events, nil
}

// LatestEventCursor returns the highest observed monotonic sequence and selection
// sequence from parsed selected-original-ready events.
func LatestEventCursor(evs []probeEventRow) (seq uint64, selSeq uint64) {
	for _, ev := range evs {
		if ev.Seq > seq {
			seq = ev.Seq
		}
		if ev.SelectionSeq > selSeq {
			selSeq = ev.SelectionSeq
		}
	}
	return seq, selSeq
}

// FilterNewOriginalReadyEvents returns events strictly newer than the given sequence
// cursor and timestamp threshold. When events carry monotonic seq/selection_seq
// metadata, stale events from prior selections are filtered out reliably.
func FilterNewOriginalReadyEvents(evs []probeEventRow, afterSeq, afterSelSeq uint64, minUnixMillis int64) []probeEventRow {
	var matched []probeEventRow
	for _, ev := range evs {
		if ev.UnixMillis < minUnixMillis {
			continue
		}
		if ev.Seq > 0 {
			if ev.Seq <= afterSeq || (afterSelSeq > 0 && ev.SelectionSeq > 0 && ev.SelectionSeq <= afterSelSeq) {
				continue
			}
		}
		matched = append(matched, ev)
	}
	return matched
}

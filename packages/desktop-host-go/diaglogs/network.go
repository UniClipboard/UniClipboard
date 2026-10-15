package diaglogs

import (
	"archive/zip"
	"crypto/sha256"
	"encoding/hex"
	"io"
	"os"
	"path"
	"path/filepath"
	"regexp"
	"slices"
	"sort"
	"time"
)

// The independent network-library log, mirrored from crates/uc-observability/src/network_archive.rs
// (same layout, same report fields). The Engine writes <logs>/network/<role>/<run_id>/ with run.json, the
// lease.lock that proves a live writer and network.<YYYYMMDD>.<n>.jsonl segments; the network root also holds
// .cleanup.lock. Only run.json and the segments are exported, so a package never carries a lock file or an
// unexpected file, and nothing is followed through a link.

const (
	networkDir          = "network"
	networkRunManifest  = "run.json"
	networkLeaseFile    = "lease.lock"
	networkCleanupLock  = ".cleanup.lock"
	networkRecentWindow = 5 * time.Minute
)

// networkRoles are the process roles the Engine names run directories after.
var networkRoles = []string{"daemon", "mobile_app", "keyboard_extension", "share_extension", "background_service"}

var (
	networkRunIDPattern   = regexp.MustCompile(`^[0-9a-f]{32}$`)
	networkSegmentPattern = regexp.MustCompile(`^network\.([0-9]{8})\.[0-9]+\.jsonl$`)
)

// Outcomes and reasons of the network block, as in the Rust report.
const (
	NetworkComplete      = "complete"
	NetworkPartial       = "partial"
	NetworkMissing       = "missing"
	NetworkNotConfigured = "notConfigured"
	NetworkUnknown       = "unknown"

	reasonEngineOffline            = "engineOffline"
	reasonFileUnreadable           = "fileUnreadable"
	reasonFileTruncated            = "fileTruncated"
	reasonOtherProcessesNotFlushed = "otherProcessesNotFlushed"
	reasonEntriesIgnored           = "entriesIgnored"
)

// NetworkFile is one exported file with the digest of exactly the bytes copied.
type NetworkFile struct {
	Name            string `json:"name"`
	Bytes           int64  `json:"bytes,string"` // a decimal string, like every counter the daemon reports
	SHA256          string `json:"sha256"`
	Truncated       bool   `json:"truncated"`
	MatchesPrepared *bool  `json:"matchesPrepared"` // always null offline: no Engine prepared a digest
}

// NetworkRun is one run directory in the package.
type NetworkRun struct {
	Role            string        `json:"role"`
	RunID           string        `json:"runId"`
	CurrentRun      bool          `json:"currentRun"` // always false offline
	Files           []NetworkFile `json:"files"`
	UnreadableFiles []string      `json:"unreadableFiles"`
}

// NetworkReport is the manifest's collection.network block. Offline nothing was flushed and no counter is known, so the
// outcome is never complete: partial when files were found, unknown when none were.
type NetworkReport struct {
	Outcome        string       `json:"outcome"`
	Reasons        []string     `json:"reasons"`
	Runs           []NetworkRun `json:"runs"`
	IgnoredEntries int          `json:"ignoredEntries"`
}

type networkCandidate struct {
	role, runID, dir string
	files            []string
}

type networkSelection struct {
	runs           []networkCandidate
	ignored        int
	unreadableRoot bool
}

func networkSegmentDay(name string) (time.Time, bool) {
	match := networkSegmentPattern.FindStringSubmatch(name)
	if match == nil {
		return time.Time{}, false
	}
	day, err := time.Parse("20060102", match[1])
	return day, err == nil
}

func networkExportedName(name string) bool {
	if name == networkRunManifest {
		return true
	}
	_, ok := networkSegmentDay(name)
	return ok
}

// selectNetwork lists the run directories to export. Directories and files that are links or not regular are counted as
// ignored, never opened.
func selectNetwork(logsDir string) networkSelection {
	var selection networkSelection
	root := filepath.Join(logsDir, networkDir)
	entries, err := os.ReadDir(root)
	if err != nil {
		if !os.IsNotExist(err) {
			selection.unreadableRoot = true
		}
		return selection
	}
	for _, entry := range entries {
		name := entry.Name()
		if name == networkCleanupLock {
			continue
		}
		if !entry.IsDir() || !slices.Contains(networkRoles, name) {
			selection.ignored++
			continue
		}
		selection.collectRole(filepath.Join(root, name), name)
	}
	sort.Slice(selection.runs, func(i, j int) bool {
		left, right := selection.runs[i], selection.runs[j]
		if left.role != right.role {
			return left.role < right.role
		}
		return left.runID < right.runID
	})
	return selection
}

func (selection *networkSelection) collectRole(roleDir, role string) {
	runs, err := os.ReadDir(roleDir)
	if err != nil {
		selection.unreadableRoot = true
		return
	}
	for _, run := range runs {
		runID := run.Name()
		if !run.IsDir() || !networkRunIDPattern.MatchString(runID) {
			selection.ignored++
			continue
		}
		dir := filepath.Join(roleDir, runID)
		files, err := os.ReadDir(dir)
		if err != nil {
			selection.unreadableRoot = true
			continue
		}
		var names []string
		for _, file := range files {
			name := file.Name()
			if name == networkLeaseFile {
				continue
			}
			if !file.Type().IsRegular() || !networkExportedName(name) {
				selection.ignored++
				continue
			}
			names = append(names, name)
		}
		if len(names) == 0 {
			continue
		}
		sort.Strings(names)
		selection.runs = append(selection.runs, networkCandidate{role: role, runID: runID, dir: dir, files: names})
	}
}

func networkRunRecentlyWritten(run networkCandidate, now time.Time) bool {
	for _, name := range run.files {
		info, err := os.Lstat(filepath.Join(run.dir, name))
		if err == nil && now.Sub(info.ModTime()) <= networkRecentWindow && now.After(info.ModTime()) {
			return true
		}
	}
	return false
}

// collectNetwork copies the selected files into archive under network/<role>/<run_id>/ and returns the report. A failure
// to write into the archive is returned; a failure to read a source file is recorded in the report.
func collectNetwork(archive *zip.Writer, selection networkSelection, now time.Time) (NetworkReport, error) {
	report := NetworkReport{Reasons: []string{}, Runs: []NetworkRun{}, IgnoredEntries: selection.ignored}
	push := func(reason string) {
		if !slices.Contains(report.Reasons, reason) {
			report.Reasons = append(report.Reasons, reason)
		}
	}
	otherRunMayBeLive := false
	collected := 0
	for _, candidate := range selection.runs {
		if networkRunRecentlyWritten(candidate, now) {
			otherRunMayBeLive = true
		}
		run := NetworkRun{Role: candidate.role, RunID: candidate.runID, Files: []NetworkFile{}, UnreadableFiles: []string{}}
		for _, name := range candidate.files {
			input, err := os.Open(filepath.Join(candidate.dir, name))
			if err != nil {
				run.UnreadableFiles = append(run.UnreadableFiles, name)
				push(reasonFileUnreadable)
				continue
			}
			info, err := input.Stat()
			if err != nil {
				input.Close()
				run.UnreadableFiles = append(run.UnreadableFiles, name)
				push(reasonFileUnreadable)
				continue
			}
			out, err := archive.CreateHeader(&zip.FileHeader{
				Name:   path.Join(networkDir, candidate.role, candidate.runID, name),
				Method: zip.Deflate,
			})
			if err != nil {
				input.Close()
				return NetworkReport{}, err
			}
			// Copy the length observed at open time so an export converges while the writer keeps appending.
			hash := sha256.New()
			copied, readErr, writeErr := copyHashing(io.LimitReader(input, info.Size()), io.MultiWriter(out, hash))
			input.Close()
			if writeErr != nil {
				return NetworkReport{}, writeErr
			}
			// A read error after the member started leaves a partial member in the zip;
			// it is listed with what was copied and also reported unreadable.
			if readErr != nil {
				run.UnreadableFiles = append(run.UnreadableFiles, name)
				push(reasonFileUnreadable)
			}
			truncated := copied != info.Size()
			if truncated {
				push(reasonFileTruncated)
			}
			run.Files = append(run.Files, NetworkFile{Name: name, Bytes: copied, SHA256: hex.EncodeToString(hash.Sum(nil)), Truncated: truncated})
			collected++
		}
		report.Runs = append(report.Runs, run)
	}
	if selection.unreadableRoot {
		push(reasonFileUnreadable)
	}
	if selection.ignored > 0 {
		push(reasonEntriesIgnored)
	}
	if otherRunMayBeLive {
		push(reasonOtherProcessesNotFlushed)
	}
	push(reasonEngineOffline)
	if collected == 0 {
		report.Outcome = NetworkUnknown
	} else {
		report.Outcome = NetworkPartial
	}
	return report, nil
}

// copyHashing copies in to out, telling a failure to read the source apart from a failure to write the archive.
func copyHashing(in io.Reader, out io.Writer) (copied int64, readErr, writeErr error) {
	buffer := make([]byte, 64*1024)
	for {
		n, err := in.Read(buffer)
		if n > 0 {
			written, werr := out.Write(buffer[:n])
			copied += int64(written)
			if werr != nil {
				return copied, nil, werr
			}
		}
		if err == io.EOF {
			return copied, nil, nil
		}
		if err != nil {
			return copied, err, nil
		}
	}
}

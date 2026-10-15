// Package diaglogs packages retained desktop logs into a diagnostic zip, matching the offline
// export of crates/uc-observability/src/startup_logs.rs (same layout and manifest fields).
package diaglogs

import (
	"archive/zip"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"sort"
	"strings"
	"time"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/apppaths"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/buildinfo"
)

// ErrNoLogs is returned when there is nothing to export.
var ErrNoLogs = errors.New("No application logs are available to export")

// source is one managed log role. A managed file name is prefix + YYYY-MM-DD + suffix; the shared fixture
// testdata/log-source-patterns.json pins these patterns for both the Go and the Rust exporters.
type source struct{ role, prefix, suffix string }

var sources = []source{
	{"gui", "uniclipboard-gui.json.", ""},
	{"daemon", "uniclipboard-daemon.json.", ""},
	{"cli", "uniclipboard-cli.json.", ""},
	{"quick-panel", "uniclipboard-quick-panel.json.", ""},
	{"engine", "engine.", ".jsonl"},
}

// undatedFiles are exact, date-less names exported alongside the managed logs (pinned in the shared fixture).
// The daemon writes daemon-startup-failure.json when a start fails before logging exists.
var undatedFiles = []string{"daemon-startup-failure.json"}

// maxUndatedFileBytes bounds how much of an undated file is exported.
const maxUndatedFileBytes = 4096

// roleOf returns the role a managed log file name belongs to, or "" for any other name.
func roleOf(name string) string {
	for _, src := range sources {
		date, ok := strings.CutPrefix(name, src.prefix)
		if !ok {
			continue
		}
		if date, ok = strings.CutSuffix(date, src.suffix); !ok {
			continue
		}
		if _, err := time.Parse("2006-01-02", date); err == nil {
			return src.role
		}
	}
	return ""
}

// Reasons a source is listed in collection.missingSources.
const (
	reasonMissing    = "missing"    // no file of the role exists in the log directory
	reasonUnreadable = "unreadable" // files exist but none could be exported
)

// MissingSource is one log role the export lacks.
type MissingSource struct {
	Source string `json:"source"`
	Reason string `json:"reason"`
}

// Report is what an export collected; it is the manifest "collection" block.
type Report struct {
	IncludedFiles            []string        `json:"includedFiles"`
	UnreadableFiles          []string        `json:"unreadableFiles"`
	TruncatedFiles           []string        `json:"truncatedFiles"`
	ConcurrentWritesPossible bool            `json:"concurrentWritesPossible"`
	MissingSources           []MissingSource `json:"missingSources"`
}

type manifest struct {
	SchemaVersion     int             `json:"schemaVersion"`
	Mode              string          `json:"mode"`
	ExportedAt        time.Time       `json:"exportedAt"`
	Since             *time.Time      `json:"since"`
	EnginePreparation json.RawMessage `json:"enginePreparation"`
	StartupStatus     json.RawMessage `json:"startupStatus"`
	Host              hostBlock       `json:"host"`
	Collection        Report          `json:"collection"`
}

// ExportStartup writes the retained logs, plus the startup status snapshot when known, to
// destination. The archive is assembled beside the destination and renamed into place, so a
// failed export leaves an existing file untouched. The returned report is the manifest's collection block. A log directory that does not exist has nothing
// to export (ErrNoLogs); any other failure to read it is returned as is.
func ExportStartup(logsDir, destination string, startupStatus json.RawMessage) (Report, error) {
	return exportStartup(environment{now: time.Now, host: currentHost}, logsDir, destination, startupStatus)
}

// environment carries the clock and host facts the manifest records, replaceable in tests.
type environment struct {
	now  func() time.Time
	host func(now time.Time) hostBlock
}

// hostBlock describes the exporting host. Facts the offline exporter cannot source cheaply (build commit, log
// profile, telemetry state) are left out rather than guessed.
type hostBlock struct {
	AppVersion            string `json:"appVersion"`
	OS                    string `json:"os"`
	Arch                  string `json:"arch"`
	Profile               string `json:"profile,omitempty"` // omitted for the default profile
	Portable              bool   `json:"portable"`
	TimezoneOffsetMinutes int    `json:"timezoneOffsetMinutes"`
}

func currentHost(now time.Time) hostBlock {
	profile, _ := apppaths.Profile()
	_, offsetSeconds := now.Zone()
	return hostBlock{
		AppVersion:            buildinfo.PackageVersion,
		OS:                    runtime.GOOS,
		Arch:                  runtime.GOARCH,
		Profile:               profile,
		Portable:              apppaths.IsPortable(),
		TimezoneOffsetMinutes: offsetSeconds / 60,
	}
}

func exportStartup(env environment, logsDir, destination string, startupStatus json.RawMessage) (Report, error) {
	names, unreadable, err := candidates(logsDir)
	if err != nil {
		return Report{}, err
	}
	if len(names) == 0 {
		return Report{}, ErrNoLogs
	}
	tmp, err := os.CreateTemp(filepath.Dir(destination), ".logs-*.zip")
	if err != nil {
		return Report{}, err
	}
	defer os.Remove(tmp.Name())
	report := Report{IncludedFiles: []string{}, UnreadableFiles: unreadable, TruncatedFiles: []string{}, ConcurrentWritesPossible: true}
	archive := zip.NewWriter(tmp)
	for _, name := range names {
		input, err := os.Open(filepath.Join(logsDir, name))
		if err != nil {
			report.UnreadableFiles = append(report.UnreadableFiles, name)
			continue
		}
		info, err := input.Stat()
		if err != nil {
			input.Close()
			report.UnreadableFiles = append(report.UnreadableFiles, name)
			continue
		}
		// Copy the length observed at open time so an export converges while writers keep appending.
		out, err := archive.CreateHeader(&zip.FileHeader{Name: "logs/" + name, Method: zip.Deflate})
		if err != nil {
			input.Close()
			return Report{}, err
		}
		limit := info.Size()
		if slices.Contains(undatedFiles, name) {
			limit = min(limit, maxUndatedFileBytes)
		}
		copied, err := io.Copy(out, io.LimitReader(input, limit))
		input.Close()
		if err != nil {
			return Report{}, err
		}
		if copied != info.Size() {
			report.TruncatedFiles = append(report.TruncatedFiles, name)
		}
		report.IncludedFiles = append(report.IncludedFiles, name)
	}
	if len(report.IncludedFiles) == 0 {
		return Report{}, ErrNoLogs
	}
	if report.UnreadableFiles == nil {
		report.UnreadableFiles = []string{}
	}
	report.MissingSources = missingSources(report.IncludedFiles, append(slices.Clone(names), report.UnreadableFiles...))
	out, err := archive.CreateHeader(&zip.FileHeader{Name: "manifest.json", Method: zip.Deflate})
	if err != nil {
		return Report{}, err
	}
	enc := json.NewEncoder(out)
	enc.SetIndent("", "  ")
	now := env.now()
	if err := enc.Encode(manifest{SchemaVersion: 1, Mode: "offline", ExportedAt: now.UTC(), StartupStatus: startupStatus, Host: env.host(now), Collection: report}); err != nil {
		return Report{}, err
	}
	if err := archive.Close(); err != nil {
		return Report{}, err
	}
	if err := tmp.Chmod(0o600); err != nil {
		return Report{}, err
	}
	if err := tmp.Sync(); err != nil {
		return Report{}, err
	}
	if err := tmp.Close(); err != nil {
		return Report{}, err
	}
	if err := os.Rename(tmp.Name(), destination); err != nil {
		return Report{}, fmt.Errorf("save log archive: %w", err)
	}
	return report, nil
}

// missingSources lists every role with no exported file: "missing" when the directory holds no file of the role,
// "unreadable" when it holds some but none could be exported. Roles are reported in the order of sources.
func missingSources(included, present []string) []MissingSource {
	out := []MissingSource{}
	for _, src := range sources {
		if slices.ContainsFunc(included, func(n string) bool { return roleOf(n) == src.role }) {
			continue
		}
		reason := reasonMissing
		if slices.ContainsFunc(present, func(n string) bool { return roleOf(n) == src.role }) {
			reason = reasonUnreadable
		}
		out = append(out, MissingSource{Source: src.role, Reason: reason})
	}
	return out
}

func candidates(logsDir string) (names, unreadable []string, err error) {
	entries, err := os.ReadDir(logsDir)
	if err != nil {
		if errors.Is(err, fs.ErrNotExist) {
			return nil, nil, nil
		}
		return nil, nil, fmt.Errorf("read log directory: %w", err)
	}
	for _, entry := range entries {
		name := entry.Name()
		if slices.Contains(undatedFiles, name) {
			if entry.Type().IsRegular() {
				names = append(names, name)
			} else {
				unreadable = append(unreadable, name)
			}
			continue
		}
		if roleOf(name) == "" {
			continue
		}
		if !entry.Type().IsRegular() {
			if entry.Type()&os.ModeSymlink != 0 || entry.IsDir() {
				continue
			}
			unreadable = append(unreadable, name)
			continue
		}
		names = append(names, name)
	}
	sort.Strings(names)
	return names, unreadable, nil
}

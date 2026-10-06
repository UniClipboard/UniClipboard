// Package diaglogs packages retained desktop logs into a diagnostic zip, matching the offline
// export of crates/uc-observability/src/startup_logs.rs (same layout and manifest fields).
package diaglogs

import (
	"archive/zip"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"time"
)

// ErrNoLogs is returned when there is nothing to export.
var ErrNoLogs = errors.New("No application logs are available to export")

var managedLog = regexp.MustCompile(`^(?:engine\.\d{4}-\d{2}-\d{2}\.jsonl|uniclipboard-(?:gui|daemon|cli)\.json\.\d{4}-\d{2}-\d{2})$`)

type collection struct {
	IncludedFiles            []string `json:"includedFiles"`
	UnreadableFiles          []string `json:"unreadableFiles"`
	TruncatedFiles           []string `json:"truncatedFiles"`
	ConcurrentWritesPossible bool     `json:"concurrentWritesPossible"`
}

type manifest struct {
	SchemaVersion     int             `json:"schemaVersion"`
	Mode              string          `json:"mode"`
	ExportedAt        time.Time       `json:"exportedAt"`
	Since             *time.Time      `json:"since"`
	EnginePreparation json.RawMessage `json:"enginePreparation"`
	StartupStatus     json.RawMessage `json:"startupStatus"`
	Collection        collection      `json:"collection"`
}

// ExportStartup writes the retained logs, plus the startup status snapshot when known, to
// destination. The archive is assembled beside the destination and renamed into place, so a
// failed export leaves an existing file untouched.
func ExportStartup(logsDir, destination string, startupStatus json.RawMessage) error {
	names, unreadable := candidates(logsDir)
	if len(names) == 0 {
		return ErrNoLogs
	}
	tmp, err := os.CreateTemp(filepath.Dir(destination), ".logs-*.zip")
	if err != nil {
		return err
	}
	defer os.Remove(tmp.Name())
	report := collection{IncludedFiles: []string{}, UnreadableFiles: unreadable, TruncatedFiles: []string{}, ConcurrentWritesPossible: true}
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
			return err
		}
		copied, err := io.Copy(out, io.LimitReader(input, info.Size()))
		input.Close()
		if err != nil {
			return err
		}
		if copied != info.Size() {
			report.TruncatedFiles = append(report.TruncatedFiles, name)
		}
		report.IncludedFiles = append(report.IncludedFiles, name)
	}
	if len(report.IncludedFiles) == 0 {
		return ErrNoLogs
	}
	if report.UnreadableFiles == nil {
		report.UnreadableFiles = []string{}
	}
	out, err := archive.CreateHeader(&zip.FileHeader{Name: "manifest.json", Method: zip.Deflate})
	if err != nil {
		return err
	}
	enc := json.NewEncoder(out)
	enc.SetIndent("", "  ")
	if err := enc.Encode(manifest{SchemaVersion: 1, Mode: "offline", ExportedAt: time.Now().UTC(), StartupStatus: startupStatus, Collection: report}); err != nil {
		return err
	}
	if err := archive.Close(); err != nil {
		return err
	}
	if err := tmp.Chmod(0o600); err != nil {
		return err
	}
	if err := tmp.Sync(); err != nil {
		return err
	}
	if err := tmp.Close(); err != nil {
		return err
	}
	if err := os.Rename(tmp.Name(), destination); err != nil {
		return fmt.Errorf("save log archive: %w", err)
	}
	return nil
}

func candidates(logsDir string) (names, unreadable []string) {
	entries, err := os.ReadDir(logsDir)
	if err != nil {
		return nil, nil
	}
	for _, entry := range entries {
		name := entry.Name()
		if !managedLog.MatchString(name) {
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
	return names, unreadable
}

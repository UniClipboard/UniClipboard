package mobile

import (
	"archive/zip"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestDiagnosticArchiveAndLogLevelOverride(t *testing.T) {
	dir := t.TempDir()
	dataDir := filepath.Join(dir, "data")
	cacheDir := filepath.Join(dir, "cache")
	privateName := "Secret-Device-Name-Never-Exported"
	cfg, err := json.Marshal(map[string]string{
		"data_dir":    dataDir,
		"cache_dir":   cacheDir,
		"device_name": privateName,
	})
	if err != nil {
		t.Fatal(err)
	}
	m, err := NewEngine(string(cfg))
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = m.Close(nil) }()
	if err := m.Start(nil); err != nil {
		t.Fatal(err)
	}

	// Default log level is info from config.
	levelRaw, err := m.GetDiagnosticLogLevelState()
	if err != nil {
		t.Fatalf("GetDiagnosticLogLevelState: %v", err)
	}
	var levelState struct {
		Local struct {
			Configured string `json:"configured_level"`
			Effective  string `json:"effective_level"`
			Source     string `json:"source"`
			Expires    *int64 `json:"expires_at_ms"`
		} `json:"local"`
	}
	if err := json.Unmarshal([]byte(levelRaw), &levelState); err != nil {
		t.Fatal(err)
	}
	if levelState.Local.Effective != "info" || levelState.Local.Source != "config" || levelState.Local.Expires != nil {
		t.Fatalf("unexpected default local level state: %s", levelRaw)
	}

	// Enable 10-minute debug override.
	overrideRaw, err := m.SetLocalLogLevelOverride("debug", 600, false)
	if err != nil {
		t.Fatalf("SetLocalLogLevelOverride(debug): %v", err)
	}
	if err := json.Unmarshal([]byte(overrideRaw), &levelState); err != nil {
		t.Fatal(err)
	}
	if levelState.Local.Effective != "debug" || levelState.Local.Source != "override" || levelState.Local.Expires == nil {
		t.Fatalf("expected active debug override, got: %s", overrideRaw)
	}

	// Export diagnostic ZIP archive with sanitized app metadata.
	metaJSON := `{"app_version":"2.0.0","app_build":"42","bundle_id":"app.uniclipboard.shell","platform":"ios","os_version":"18.4"}`
	archiveRaw, err := m.ExportDiagnosticArchive(metaJSON)
	if err != nil {
		t.Fatalf("ExportDiagnosticArchive: %v", err)
	}
	var archiveInfo mobileDiagnosticArchive
	if err := json.Unmarshal([]byte(archiveRaw), &archiveInfo); err != nil {
		t.Fatal(err)
	}
	if archiveInfo.MIME != "application/zip" || !strings.HasSuffix(archiveInfo.FileName, ".zip") {
		t.Fatalf("unexpected archive metadata: %+v", archiveInfo)
	}
	if archiveInfo.LogFileCount < 1 || len(archiveInfo.Warnings) != 0 {
		t.Fatalf("expected disk engine logs without warnings, got: %+v", archiveInfo)
	}

	// Verify ZIP contents, SHA-256 manifest, and zero privacy leaks.
	zr, err := zip.OpenReader(archiveInfo.Path)
	if err != nil {
		t.Fatalf("zip.OpenReader: %v", err)
	}
	defer func() { _ = zr.Close() }()

	entries := make(map[string][]byte)
	for _, f := range zr.File {
		rc, err := f.Open()
		if err != nil {
			t.Fatalf("open zip entry %s: %v", f.Name, err)
		}
		body, err := io.ReadAll(rc)
		_ = rc.Close()
		if err != nil {
			t.Fatalf("read zip entry %s: %v", f.Name, err)
		}
		if strings.Contains(string(body), privateName) || strings.Contains(string(body), dataDir) || strings.Contains(string(body), cacheDir) {
			t.Fatalf("private data leaked into zip entry %s", f.Name)
		}
		entries[f.Name] = body
	}

	manifestBody, ok := entries["manifest.json"]
	if !ok {
		t.Fatalf("missing manifest.json in archive")
	}
	var manifest mobileDiagnosticManifest
	if err := json.Unmarshal(manifestBody, &manifest); err != nil {
		t.Fatalf("decode manifest.json: %v", err)
	}
	if manifest.FormatVersion != 1 || manifest.Service.Name != "unic-mobile" {
		t.Fatalf("unexpected manifest header: %+v", manifest)
	}
	if manifest.App.AppVersion != "2.0.0" || manifest.App.Platform != "ios" {
		t.Fatalf("unexpected app metadata: %+v", manifest.App)
	}
	if manifest.Diagnostics.LogLevels.Local.Effective != "debug" || manifest.Diagnostics.LogLevels.Local.Source != "override" {
		t.Fatalf("manifest did not capture active debug override: %+v", manifest.Diagnostics.LogLevels)
	}
	if manifest.EngineLogs.Status != "included" || len(manifest.Files) < 1 {
		t.Fatalf("expected included engine logs, got: %+v", manifest.EngineLogs)
	}
	foundStarted := false
	for _, f := range manifest.Files {
		body, exists := entries[f.Name]
		if !exists {
			t.Fatalf("manifest references missing zip entry %s", f.Name)
		}
		sum := sha256.Sum256(body)
		if int64(len(body)) != f.Bytes || hex.EncodeToString(sum[:]) != f.SHA256 {
			t.Fatalf("checksum mismatch for %s", f.Name)
		}
		if strings.Contains(string(body), "engine.started") {
			foundStarted = true
		}
		for _, line := range strings.Split(strings.TrimSpace(string(body)), "\n") {
			if !json.Valid([]byte(line)) {
				t.Fatalf("invalid jsonl line in %s: %q", f.Name, line)
			}
		}
	}
	if !foundStarted {
		t.Fatalf("expected engine.started record in exported engine logs")
	}

	// Reset local override back to config.
	resetRaw, err := m.SetLocalLogLevelOverride("", 0, true)
	if err != nil {
		t.Fatalf("SetLocalLogLevelOverride(reset): %v", err)
	}
	if err := json.Unmarshal([]byte(resetRaw), &levelState); err != nil {
		t.Fatal(err)
	}
	if levelState.Local.Effective != "info" || levelState.Local.Source != "config" || levelState.Local.Expires != nil {
		t.Fatalf("expected reset local level state, got: %s", resetRaw)
	}
}

func TestDiagnosticArchiveRejectsSymlinkLogs(t *testing.T) {
	dir := t.TempDir()
	dataDir := filepath.Join(dir, "data")
	cacheDir := filepath.Join(dir, "cache")
	secretFile := filepath.Join(dir, "secret.txt")
	if err := os.WriteFile(secretFile, []byte("top-secret-host-file"), 0600); err != nil {
		t.Fatal(err)
	}
	cfg, _ := json.Marshal(map[string]string{"data_dir": dataDir, "cache_dir": cacheDir})
	m, err := NewEngine(string(cfg))
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = m.Close(nil) }()
	if err := m.Start(nil); err != nil {
		t.Fatal(err)
	}
	_ = m.diagnostics.controller.FlushLocal(nil)

	// Place a symlink inside logs/ matching segment-*.jsonl.
	linkPath := filepath.Join(dataDir, "logs", "segment-999999999999999999.jsonl")
	if err := os.Symlink(secretFile, linkPath); err != nil {
		t.Skipf("symlink unavailable: %v", err)
	}
	archiveRaw, err := m.ExportDiagnosticArchive("")
	if err != nil {
		t.Fatalf("ExportDiagnosticArchive: %v", err)
	}
	var archiveInfo mobileDiagnosticArchive
	if err := json.Unmarshal([]byte(archiveRaw), &archiveInfo); err != nil {
		t.Fatal(err)
	}
	zr, err := zip.OpenReader(archiveInfo.Path)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = zr.Close() }()
	for _, f := range zr.File {
		rc, err := f.Open()
		if err != nil {
			t.Fatal(err)
		}
		body, _ := io.ReadAll(rc)
		_ = rc.Close()
		if strings.Contains(string(body), "top-secret-host-file") {
			t.Fatalf("symlink target leaked into archive entry %s", f.Name)
		}
	}
	if len(archiveInfo.Warnings) == 0 || archiveInfo.Warnings[0] != "engine_logs_unreadable" {
		t.Fatalf("expected engine_logs_unreadable warning for symlink segment, got: %+v", archiveInfo.Warnings)
	}
}

// Command mobile-diagnostics verifies local diagnostics, temporary debug capture,
// and ZIP diagnostic bundle export through two mobile Engine processes.
package main

import (
	"archive/zip"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"

	"github.com/uniclipboard/unic/bindings/mobile"
)

const privateMarker = "private mobile diagnostic fixture never exported"

func must[T any](value T, err error) T {
	check(err)
	return value
}

func check(err error) {
	if err != nil {
		panic(err)
	}
}

func require(ok bool, message string) {
	if !ok {
		panic(message)
	}
}

func configured(state string) bool {
	var value struct {
		Configured bool `json:"configured_enabled"`
	}
	check(json.Unmarshal([]byte(state), &value))
	return value.Configured
}

func verifyArchive(m *mobile.Engine, profile, cacheDir string) {
	levelJSON := must(m.SetLocalLogLevelOverride("debug", 600, false))
	var levelState struct {
		Local struct {
			Effective string `json:"effective_level"`
			Source    string `json:"source"`
			Expires   *int64 `json:"expires_at_ms"`
		} `json:"local"`
	}
	check(json.Unmarshal([]byte(levelJSON), &levelState))
	require(levelState.Local.Effective == "debug" && levelState.Local.Source == "override" && levelState.Local.Expires != nil,
		"local debug capture override did not activate")

	archiveJSON := must(m.ExportDiagnosticArchive(`{"app_version":"2.0.0","app_build":"1","bundle_id":"app.uniclipboard.shell","platform":"mobile","os_version":"test"}`))
	var archive struct {
		Directory    string   `json:"directory"`
		Path         string   `json:"path"`
		FileName     string   `json:"file_name"`
		MIME         string   `json:"mime"`
		SizeBytes    int64    `json:"size_bytes"`
		LogFileCount int      `json:"log_file_count"`
		Warnings     []string `json:"warnings"`
	}
	check(json.Unmarshal([]byte(archiveJSON), &archive))
	defer func() { _ = os.RemoveAll(archive.Directory) }()
	require(archive.MIME == "application/zip" && strings.HasSuffix(archive.FileName, ".zip"), "unexpected diagnostic archive mime/name")
	require(archive.SizeBytes > 0 && archive.LogFileCount >= 1 && len(archive.Warnings) == 0, "expected diagnostic archive with disk logs and no warnings")

	zr := must(zip.OpenReader(archive.Path))
	defer func() { _ = zr.Close() }()
	entries := map[string][]byte{}
	for _, f := range zr.File {
		rc := must(f.Open())
		body := must(io.ReadAll(rc))
		_ = rc.Close()
		text := string(body)
		require(!strings.Contains(text, privateMarker) && !strings.Contains(text, profile) && !strings.Contains(text, cacheDir),
			"private data reached diagnostic archive")
		entries[f.Name] = body
	}
	manifestBytes, ok := entries["manifest.json"]
	require(ok, "diagnostic archive missing manifest.json")
	var manifest struct {
		FormatVersion int `json:"format_version"`
		EngineLogs    struct {
			Status string `json:"status"`
		} `json:"engine_logs"`
		Files []struct {
			Name   string `json:"name"`
			Bytes  int64  `json:"bytes"`
			SHA256 string `json:"sha256"`
		} `json:"files"`
	}
	check(json.Unmarshal(manifestBytes, &manifest))
	require(manifest.FormatVersion == 1 && manifest.EngineLogs.Status == "included" && len(manifest.Files) >= 1,
		"diagnostic archive manifest incomplete")
	foundStarted := false
	for _, entry := range manifest.Files {
		body, exists := entries[entry.Name]
		require(exists, "manifest file missing from zip")
		sum := sha256.Sum256(body)
		require(int64(len(body)) == entry.Bytes && hex.EncodeToString(sum[:]) == entry.SHA256, "diagnostic file digest mismatch")
		if strings.Contains(string(body), "engine.started") {
			foundStarted = true
		}
		for _, line := range strings.Split(strings.TrimSpace(string(body)), "\n") {
			require(json.Valid([]byte(line)), "diagnostic log entry must contain complete JSON lines")
		}
	}
	require(foundStarted, "diagnostic archive missing engine.started record")

	resetJSON := must(m.SetLocalLogLevelOverride("", 0, true))
	check(json.Unmarshal([]byte(resetJSON), &levelState))
	require(levelState.Local.Source == "config" && levelState.Local.Expires == nil, "local debug capture override did not reset")
}

func process(profile, cacheDir string, writer bool) {
	config := must(json.Marshal(map[string]string{"data_dir": profile, "cache_dir": cacheDir, "device_name": privateMarker}))
	m := must(mobile.NewEngine(string(config)))
	defer func() { check(m.Close(nil)) }()
	check(m.Start(nil))
	require(configured(must(m.GetTelemetryState())) == writer, "unexpected telemetry preference after launch")
	if writer {
		require(!configured(must(m.SetTelemetryEnabled(false))), "disabling telemetry did not apply")
	}
	deadline := time.Now().Add(4 * time.Second)
	for {
		logs := m.ExportRecentDiagnosticLogs()
		if strings.Contains(logs, "engine.started") {
			require(!strings.Contains(logs, privateMarker) && !strings.Contains(logs, profile), "private data reached local diagnostic export")
			for _, line := range strings.Split(strings.TrimSpace(logs), "\n") {
				require(json.Valid([]byte(line)), "diagnostics must contain complete JSON lines")
			}
			break
		}
		require(time.Now().Before(deadline), "local log export was unavailable")
		time.Sleep(20 * time.Millisecond)
	}
	verifyArchive(m, profile, cacheDir)
	if !writer {
		require(configured(must(m.SetTelemetryEnabled(true))), "enabling telemetry did not apply")
	}
}

func main() {
	if len(os.Args) == 4 {
		process(os.Args[2], os.Args[3], os.Args[1] == "write")
		return
	}
	dir := must(os.MkdirTemp("", "unic-mobile-diagnostics-"))
	defer os.RemoveAll(dir)
	resolved := must(filepath.EvalSymlinks(dir))
	profile := filepath.Join(resolved, "profile")
	cacheDir := filepath.Join(resolved, "cache")
	executable := must(os.Executable())
	for _, phase := range []string{"write", "read"} {
		cmd := exec.Command(executable, phase, profile, cacheDir)
		cmd.Stdout, cmd.Stderr = os.Stdout, os.Stderr
		check(cmd.Run())
	}
	fmt.Println(`{"default_enabled":true,"disable_persisted":true,"reenabled":true,"local_log_export":"zip_bundle","debug_capture_override":true}`)
}

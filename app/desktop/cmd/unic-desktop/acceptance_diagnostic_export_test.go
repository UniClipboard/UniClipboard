package main

import (
	"archive/zip"
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"testing"
)

func scenarioDesktopDiagnosticExport(t *testing.T) {
	executable, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	cli := "unic"
	if runtime.GOOS == "windows" {
		cli += ".exe"
	}
	if _, err = os.Lstat(filepath.Join(filepath.Dir(executable), cli)); !os.IsNotExist(err) {
		t.Fatalf("acceptance requires no sibling CLI: %v", err)
	}
	dir := t.TempDir()
	profile := filepath.Join(dir, "profile with spaces")
	logs := filepath.Join(profile, "logs")
	if err = os.MkdirAll(logs, 0700); err != nil {
		t.Fatal(err)
	}
	log := []byte("{\"level\":\"INFO\",\"msg\":\"engine.started\"}\n")
	receipt := []byte(`{"status":"pending"}`)
	for name, data := range map[string][]byte{
		filepath.Join(logs, "segment-1.jsonl"):       log,
		filepath.Join(profile, "cli-migration.json"): receipt,
		filepath.Join(profile, "config.toml"):        []byte("private-config-canary"),
	} {
		if err = os.WriteFile(name, data, 0600); err != nil {
			t.Fatal(err)
		}
	}
	output := filepath.Join(dir, "desktop diagnostics.zip")
	if saved, err := exportDesktopDiagnosticLogs(t.Context(), profile, output, nil); err != nil || saved != output {
		t.Fatalf("Desktop diagnostic export failed: %v", err)
	}
	if after, err := os.ReadFile(filepath.Join(profile, "cli-migration.json")); err != nil || !bytes.Equal(after, receipt) {
		t.Fatal("Desktop export changed the CLI migration receipt")
	}
	archive, err := zip.OpenReader(output)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = archive.Close() }()
	if len(archive.File) != 2 {
		t.Fatalf("expected exactly one log and the manifest, got %d entries", len(archive.File))
	}
	read := func(name string) []byte {
		t.Helper()
		file, err := archive.Open(name)
		if err != nil {
			t.Fatal(err)
		}
		defer func() { _ = file.Close() }()
		data, err := io.ReadAll(file)
		if err != nil {
			t.Fatal(err)
		}
		return data
	}
	const member = "logs/engine/segment-1.jsonl"
	if data := read(member); !bytes.Equal(data, log) {
		t.Fatal("Desktop archive log differs from the retained log")
	}
	var manifest struct {
		FormatVersion   int    `json:"format_version"`
		ServiceSnapshot string `json:"service_snapshot"`
		Service         struct {
			Name string `json:"name"`
		} `json:"service"`
		Files []struct {
			Name   string `json:"name"`
			Bytes  int    `json:"bytes"`
			SHA256 string `json:"sha256"`
		} `json:"files"`
	}
	raw := read("manifest.json")
	if err = json.Unmarshal(raw, &manifest); err != nil {
		t.Fatal(err)
	}
	if manifest.FormatVersion != 1 || manifest.Service.Name != "unic-desktop" || manifest.ServiceSnapshot != "offline" || len(manifest.Files) != 1 {
		t.Fatalf("unexpected Desktop archive manifest: %s", raw)
	}
	logHash := sha256.Sum256(log)
	file := manifest.Files[0]
	if file.Name != member || file.Bytes != len(log) || file.SHA256 != hex.EncodeToString(logHash[:]) {
		t.Fatalf("unexpected log checksum in Desktop archive: %+v", file)
	}
	if _, err = exportDesktopDiagnosticLogs(t.Context(), profile, output, nil); err == nil {
		t.Fatal("Desktop export accepted an existing destination")
	}
	if out := os.Getenv("UNIC_DESKTOP_E2E_CASE_RECEIPT"); out != "" {
		if err = os.WriteFile(out, raw, 0600); err != nil {
			t.Fatal(err)
		}
	}
}

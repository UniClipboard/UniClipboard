package e2e

import (
	"archive/zip"
	"bytes"
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"net"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"testing"
	"time"
)

// Real production CLI + filesystem scenarios, written before the exporter.
// The daemon socket is absent for offline cases; an isolated local socket tests
// best-effort live status attachment without starting a service.
func TestLogsExport(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	executable := filepath.Join(binDir, "unic")
	if _, err := os.Stat(executable); err != nil {
		t.Fatal("missing CLI binary")
	}
	results := map[string]string{}
	source := logsExportSources(t)
	var archiveHashes = map[string]string{}
	run := func(name string, fn func(*testing.T, string, string)) {
		passed := t.Run(name, func(t *testing.T) {
			dir := t.TempDir()
			home := filepath.Join(dir, "profile with spaces")
			if err := os.MkdirAll(filepath.Join(home, "logs"), 0700); err != nil {
				t.Fatal(err)
			}
			if err := initializeFixtureProfile(executable, home, append(os.Environ(), "UNIC_SOCK="+filepath.Join(dir, "absent.sock"))); err != nil {
				t.Fatal(err)
			}
			fn(t, dir, home)
		})
		results[name] = "PASS"
		if !passed {
			results[name] = "FAIL"
		}
	}
	write := func(t *testing.T, path string, data []byte) {
		t.Helper()
		if err := os.WriteFile(path, data, 0600); err != nil {
			t.Fatal(err)
		}
	}
	line := []byte("{\"level\":\"INFO\",\"msg\":\"engine.started\"}\n")
	run("offline-private-archive", func(t *testing.T, dir, home string) {
		for _, name := range []string{"segment-11.jsonl", "segment-22.jsonl"} {
			write(t, filepath.Join(home, "logs", name), line)
		}
		for _, path := range []string{"config.toml", "analytics-id", "logs/private.txt"} {
			write(t, filepath.Join(home, path), []byte("must-not-export-private-canary"))
		}
		out := filepath.Join(dir, "support logs.zip")
		stdout, err := logsExportCLI(t, executable, dir, home, "dev", "logs", "export", "--output", out)
		if err != nil {
			t.Fatalf("offline export failed: %s", stdout)
		}
		if strings.TrimSpace(stdout) != out {
			t.Fatal("stdout must be absolute archive path")
		}
		entries := readLogArchive(t, out)
		if len(entries) != 3 || !bytes.Equal(entries["logs/engine/segment-11.jsonl"], line) || !bytes.Equal(entries["logs/engine/segment-22.jsonl"], line) {
			t.Fatal("archive did not contain exactly the selected logs and manifest")
		}
		assertLogManifest(t, entries, "offline")
		for _, data := range entries {
			if bytes.Contains(data, []byte(home)) || bytes.Contains(data, []byte("must-not-export")) {
				t.Fatal("private non-log data in archive")
			}
		}
		if runtime.GOOS != "windows" {
			info, _ := os.Stat(out)
			if info.Mode().Perm() != 0600 {
				t.Fatal("archive permissions")
			}
		}
		data, _ := os.ReadFile(out)
		sum := sha256.Sum256(data)
		archiveHashes["offline"] = hex.EncodeToString(sum[:])
	})
	run("live-service-summary-redacted", func(t *testing.T, dir, home string) {
		write(t, filepath.Join(home, "logs", "segment-11.jsonl"), line)
		sock := filepath.Join(dir, "live.sock")
		ln, err := net.Listen("unix", sock)
		if err != nil {
			t.Fatal(err)
		}
		defer func() { _ = ln.Close() }()
		mux := http.NewServeMux()
		mux.HandleFunc("/v1/status", func(w http.ResponseWriter, _ *http.Request) {
			_ = json.NewEncoder(w).Encode(map[string]any{
				"id":   "secret-self-id-canary",
				"name": "Secret-Host-Name-Canary",
				"peers": []map[string]any{
					{"id": "secret-peer-1", "name": "Secret-Peer-Name", "fingerprint": "ABCD-1234", "connected": true},
					{"id": "secret-peer-2", "name": "Offline-Peer-Name", "connected": false},
					{"id": "ext-1", "name": "External-Channel", "kind": "external", "connected": false},
				},
				"pending": []map[string]any{
					{"id": "pending-1", "name": "Pending-Secret-Peer"},
				},
			})
		})
		mux.HandleFunc("/v1/capabilities", func(w http.ResponseWriter, _ *http.Request) {
			_ = json.NewEncoder(w).Encode(map[string]any{
				"effective": map[string]any{
					"observe":     "available",
					"apply":       "available",
					"read_files":  true,
					"write_files": true,
				},
			})
		})
		mux.HandleFunc("/v1/config", func(w http.ResponseWriter, _ *http.Request) {
			_ = json.NewEncoder(w).Encode(map[string]any{
				"config": map[string]any{
					"device_name": "Secret-Config-Device-Canary",
					"inbox":       "/secret/inbox/path/canary",
					"sync": map[string]any{
						"enabled":    true,
						"automatic":  true,
						"files":      true,
						"on_restore": false,
					},
				},
			})
		})
		mux.HandleFunc("/v1/diagnostics/telemetry", func(w http.ResponseWriter, _ *http.Request) {
			_ = json.NewEncoder(w).Encode(map[string]any{
				"configured_enabled":    true,
				"effective_enabled":     true,
				"restart_required":      []string{},
				"credential_configured": false,
				"logs":                  map[string]any{"requested_enabled": true, "state": "active", "last_error_code": nil, "dropped_records": 0, "export_failures": 0},
				"metrics":               map[string]any{"requested_enabled": true, "state": "active", "last_error_code": nil, "dropped_records": 0, "export_failures": 0},
				"analytics":             map[string]any{"requested_enabled": true, "state": "active", "last_error_code": nil, "dropped_records": 0, "export_failures": 0, "pending_records": 0},
				"traces":                map[string]any{"requested_enabled": true, "state": "active", "last_error_code": nil, "dropped_records": 0, "export_failures": 0},
			})
		})
		mux.HandleFunc("/v1/diagnostics/log-level", func(w http.ResponseWriter, _ *http.Request) {
			_ = json.NewEncoder(w).Encode(map[string]any{
				"local":               map[string]any{"configured_level": "info", "effective_level": "debug", "source": "override", "expires_at_ms": 1794226500000, "dropped_records": 0},
				"remote":              map[string]any{"configured_level": "warn", "effective_level": "warn", "source": "config", "expires_at_ms": nil, "dropped_records": 0},
				"telemetry_enabled":   true,
				"remote_logs_enabled": true,
			})
		})
		srv := &http.Server{Handler: mux}
		go func() { _ = srv.Serve(ln) }()
		defer func() { _ = srv.Close() }()

		out := filepath.Join(dir, "live-summary.zip")
		ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
		defer cancel()
		cmd := exec.CommandContext(ctx, executable, "dev", "logs", "export", "--output", out)
		cmd.Dir = dir
		cmd.Env = append(os.Environ(), "UNIC_HOME="+home, "UNIC_SOCK="+sock)
		stdout, err := cmd.CombinedOutput()
		if err != nil {
			t.Fatalf("live summary export failed: %s", stdout)
		}
		entries := readLogArchive(t, out)
		assertLogManifest(t, entries, "live")
		manifestRaw := string(entries["manifest.json"])
		for _, secret := range []string{"Secret-Host-Name-Canary", "secret-self-id-canary", "Secret-Peer-Name", "ABCD-1234", "Secret-Config-Device-Canary", "/secret/inbox/path/canary", "Pending-Secret-Peer"} {
			if strings.Contains(manifestRaw, secret) {
				t.Fatalf("sensitive live field %q leaked into manifest.json", secret)
			}
		}
		var liveManifest struct {
			Runtime struct {
				PairedDeviceCount    int  `json:"paired_device_count"`
				ConnectedDeviceCount int  `json:"connected_device_count"`
				PendingDeviceCount   int  `json:"pending_device_count"`
				SyncEnabled          bool `json:"sync_enabled"`
			} `json:"runtime"`
			Diagnostics struct {
				LogLevels struct {
					Local struct {
						Effective string `json:"effective_level"`
						Source    string `json:"source"`
					} `json:"local"`
				} `json:"log_levels"`
			} `json:"diagnostics"`
		}
		if err := json.Unmarshal(entries["manifest.json"], &liveManifest); err != nil {
			t.Fatal(err)
		}
		if liveManifest.Runtime.PairedDeviceCount != 2 || liveManifest.Runtime.ConnectedDeviceCount != 1 || liveManifest.Runtime.PendingDeviceCount != 1 || !liveManifest.Runtime.SyncEnabled {
			t.Fatalf("unexpected runtime counts: %+v", liveManifest.Runtime)
		}
		if liveManifest.Diagnostics.LogLevels.Local.Effective != "debug" || liveManifest.Diagnostics.LogLevels.Local.Source != "override" {
			t.Fatalf("unexpected live log levels: %+v", liveManifest.Diagnostics.LogLevels)
		}
	})
	run("default-output", func(t *testing.T, dir, home string) {
		write(t, filepath.Join(home, "logs", "segment-1.jsonl"), line)
		a, err := logsExportCLI(t, executable, dir, home, "dev", "logs", "export")
		if err != nil {
			t.Fatal("default export failed")
		}
		b, err := logsExportCLI(t, executable, dir, home, "dev", "logs", "export")
		if err != nil {
			t.Fatal("second default export failed")
		}
		pathA, pathB := strings.TrimSpace(a), strings.TrimSpace(b)
		if pathA == pathB || !filepath.IsAbs(pathA) || !strings.HasSuffix(pathA, ".zip") || !strings.HasPrefix(filepath.Base(pathA), "unic-diagnostics-") {
			t.Fatalf("default names must be distinct absolute unic-diagnostics-*.zip paths: %q %q", pathA, pathB)
		}
		assertLogManifest(t, readLogArchive(t, pathA), "offline")
	})
	for _, kind := range []string{"missing", "empty", "directory", "segment-symlink", "logs-symlink", "oversize", "total-limit", "inside-logs", "bad-parent"} {
		run(kind, func(t *testing.T, dir, home string) {
			logdir := filepath.Join(home, "logs")
			out := filepath.Join(dir, "failed.zip")
			switch kind {
			case "missing":
				if err := os.Remove(logdir); err != nil {
					t.Fatal(err)
				}
			case "empty":
				write(t, filepath.Join(logdir, "unrelated.txt"), line)
			case "directory":
				if err := os.Mkdir(filepath.Join(logdir, "segment-1.jsonl"), 0700); err != nil {
					t.Fatal(err)
				}
			case "segment-symlink":
				target := filepath.Join(dir, "private")
				write(t, target, line)
				if err := os.Symlink(target, filepath.Join(logdir, "segment-1.jsonl")); err != nil {
					t.Fatal(err)
				}
			case "logs-symlink":
				if err := os.Remove(logdir); err != nil {
					t.Fatal(err)
				}
				outside := filepath.Join(dir, "outside")
				if err := os.Mkdir(outside, 0700); err != nil {
					t.Fatal(err)
				}
				write(t, filepath.Join(outside, "segment-1.jsonl"), line)
				if err := os.Symlink(outside, logdir); err != nil {
					t.Fatal(err)
				}
			case "oversize", "total-limit":
				count, size := 1, int64(4<<20)+1
				if kind == "total-limit" {
					count, size = 6, 4<<20
				}
				for i := 0; i < count; i++ {
					f, err := os.Create(filepath.Join(logdir, fmt.Sprintf("segment-%d.jsonl", i+1)))
					if err != nil {
						t.Fatal(err)
					}
					if err := f.Truncate(size); err != nil {
						t.Fatal(err)
					}
					_ = f.Close()
				}
			case "inside-logs":
				write(t, filepath.Join(logdir, "segment-1.jsonl"), line)
				out = filepath.Join(logdir, "export.zip")
			case "bad-parent":
				write(t, filepath.Join(logdir, "segment-1.jsonl"), line)
				out = filepath.Join(dir, "absent", "export.zip")
			}
			_, err := logsExportCLI(t, executable, dir, home, "dev", "logs", "export", "--output", out)
			if err == nil {
				t.Fatal("invalid export succeeded")
			}
			if _, err := os.Lstat(out); !os.IsNotExist(err) {
				t.Fatal("failed export retained an archive")
			}
		})
	}
	run("exclusive-output", func(t *testing.T, dir, home string) {
		write(t, filepath.Join(home, "logs", "segment-1.jsonl"), line)
		target := filepath.Join(dir, "existing.zip")
		write(t, target, []byte("keep-original"))
		for _, out := range []string{target, filepath.Join(dir, "output-link.zip")} {
			if out != target {
				if err := os.Symlink(target, out); err != nil {
					t.Fatal(err)
				}
			}
			if _, err := logsExportCLI(t, executable, dir, home, "dev", "logs", "export", "--output", out); err == nil {
				t.Fatal("existing destination accepted")
			}
			data, _ := os.ReadFile(target)
			if string(data) != "keep-original" {
				t.Fatal("destination overwritten")
			}
		}
	})
	run("invalid-arguments", func(t *testing.T, dir, home string) {
		for _, args := range [][]string{{"dev", "logs", "export", "extra"}, {"dev", "logs", "export", "--unknown"}, {"dev", "logs", "export", "--output"}, {"dev", "logs", "export", "--output", ""}} {
			if _, err := logsExportCLI(t, executable, dir, home, args...); err == nil {
				t.Fatal("invalid arguments accepted")
			}
		}
	})
	run("live-append-bounded-snapshot", func(t *testing.T, dir, home string) {
		path := filepath.Join(home, "logs", "segment-1.jsonl")
		initial := bytes.Repeat(line, (2<<20)/len(line))
		write(t, path, initial)
		f, err := os.OpenFile(path, os.O_WRONLY|os.O_APPEND, 0)
		if err != nil {
			t.Fatal(err)
		}
		stop := make(chan struct{})
		var wg sync.WaitGroup
		wg.Add(1)
		go func() {
			defer wg.Done()
			defer func() { _ = f.Close() }()
			for i := 0; i < 1024; i++ {
				select {
				case <-stop:
					return
				default:
				}
				if _, err := f.Write(line); err != nil {
					return
				}
				time.Sleep(time.Millisecond)
			}
		}()
		out := filepath.Join(dir, "live.zip")
		_, exportErr := logsExportCLI(t, executable, dir, home, "dev", "logs", "export", "--output", out)
		close(stop)
		wg.Wait()
		if exportErr != nil {
			t.Fatal("live export failed")
		}
		entries := readLogArchive(t, out)
		assertLogManifest(t, entries, "offline")
		data := entries["logs/engine/segment-1.jsonl"]
		info, _ := os.Stat(path)
		if !bytes.HasPrefix(data, initial) || int64(len(data)) > info.Size() {
			t.Fatal("invalid append snapshot")
		}
	})

	run("truncated-source-removes-partial-archive", func(t *testing.T, dir, home string) {
		path := filepath.Join(home, "logs", "segment-1.jsonl")
		payload := make([]byte, 4<<20)
		if _, err := rand.Read(payload); err != nil {
			t.Fatal(err)
		}
		write(t, path, payload)
		out := filepath.Join(dir, "truncated.zip")
		stop := make(chan struct{})
		result := make(chan error, 1)
		go func() {
			for {
				select {
				case <-stop:
					result <- fmt.Errorf("export ended before truncation")
					return
				default:
				}
				if _, err := os.Stat(out); err == nil {
					result <- os.Truncate(path, 0)
					return
				}
				time.Sleep(100 * time.Microsecond)
			}
		}()
		_, err := logsExportCLI(t, executable, dir, home, "dev", "logs", "export", "--output", out)
		close(stop)
		if err == nil {
			t.Fatal("truncated source export succeeded")
		}
		if err := <-result; err != nil {
			t.Fatal(err)
		}
		if _, err := os.Lstat(out); !os.IsNotExist(err) {
			t.Fatal("partial archive retained after read failure")
		}
	})
	if !sameHashes(source, logsExportSources(t)) {
		t.Error("source changed during E2E")
	}
	if dest := os.Getenv("UNIC_E2E_ARTIFACTS"); dest != "" {
		dest = filepath.Join(dest, "logs-export")
		if err := os.MkdirAll(dest, 0700); err != nil {
			t.Fatal(err)
		}
		binary, _ := os.ReadFile(executable)
		sum := sha256.Sum256(binary)
		data, _ := json.MarshalIndent(map[string]any{"scenarios": results, "failed": t.Failed(), "source_files_sha256": source, "binary_sha256": hex.EncodeToString(sum[:]), "archive_sha256": archiveHashes, "go": runtime.Version(), "transport": "offline filesystem plus optional read-only local socket", "replay": "make logs-export-e2e"}, "", "  ")
		if err := os.WriteFile(filepath.Join(dest, "manifest.json"), data, 0600); err != nil {
			t.Fatal(err)
		}
		replay := []byte("#!/bin/sh\nset -eu\ncd \"$(git rev-parse --show-toplevel)\"\nmake logs-export-e2e\n")
		if err := os.WriteFile(filepath.Join(dest, "replay.sh"), replay, 0700); err != nil {
			t.Fatal(err)
		}
		var sums strings.Builder
		for _, name := range []string{"manifest.json", "replay.sh"} {
			b, _ := os.ReadFile(filepath.Join(dest, name))
			sum := sha256.Sum256(b)
			fmt.Fprintf(&sums, "%x  %s\n", sum, name)
		}
		if err := os.WriteFile(filepath.Join(dest, "SHA256SUMS"), []byte(sums.String()), 0600); err != nil {
			t.Fatal(err)
		}
	}
}

func logsExportCLI(t *testing.T, executable, dir, home string, args ...string) (string, error) {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	cmd := exec.CommandContext(ctx, executable, args...)
	cmd.Dir = dir
	cmd.Env = append(os.Environ(), "UNIC_HOME="+home, "UNIC_SOCK="+filepath.Join(dir, "absent.sock"))
	out, err := cmd.CombinedOutput()
	return string(out), err
}

func readLogArchive(t *testing.T, path string) map[string][]byte {
	t.Helper()
	zr, err := zip.OpenReader(path)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = zr.Close() }()
	entries := map[string][]byte{}
	for _, f := range zr.File {
		if !f.Mode().IsRegular() || f.Mode().Perm() != 0600 || strings.Contains(f.Name, "..") || strings.HasPrefix(f.Name, "/") {
			t.Fatalf("unsafe archive entry %q mode %v", f.Name, f.Mode())
		}
		if _, ok := entries[f.Name]; ok {
			t.Fatal("duplicate archive entry")
		}
		rc, err := f.Open()
		if err != nil {
			t.Fatal(err)
		}
		data, err := io.ReadAll(io.LimitReader(rc, 5<<20))
		_ = rc.Close()
		if err != nil {
			t.Fatal(err)
		}
		entries[f.Name] = data
	}
	return entries
}

func assertLogManifest(t *testing.T, entries map[string][]byte, wantSnapshot string) {
	t.Helper()
	var manifest struct {
		FormatVersion   int    `json:"format_version"`
		ServiceSnapshot string `json:"service_snapshot"`
		Service         struct {
			Name    string `json:"name"`
			Version string `json:"version"`
			OS      string `json:"os"`
			Arch    string `json:"arch"`
		} `json:"service"`
		EngineLogs struct {
			Status          string `json:"status"`
			DiscoveredFiles int    `json:"discovered_files"`
			IncludedFiles   int    `json:"included_files"`
		} `json:"engine_logs"`
		Files []struct {
			Name   string `json:"name"`
			Bytes  int64  `json:"bytes"`
			SHA256 string `json:"sha256"`
		} `json:"files"`
	}
	if json.Unmarshal(entries["manifest.json"], &manifest) != nil || manifest.FormatVersion != 1 || len(manifest.Files) != len(entries)-1 {
		t.Fatalf("invalid archive manifest: %s", entries["manifest.json"])
	}
	if manifest.Service.Name != "unic" || manifest.Service.OS == "" || manifest.Service.Arch == "" {
		t.Fatalf("missing service metadata in manifest: %+v", manifest.Service)
	}
	if manifest.ServiceSnapshot != wantSnapshot {
		t.Fatalf("expected service_snapshot=%q, got %q", wantSnapshot, manifest.ServiceSnapshot)
	}
	if manifest.EngineLogs.Status != "included" || manifest.EngineLogs.IncludedFiles != len(manifest.Files) {
		t.Fatalf("invalid engine_logs summary: %+v", manifest.EngineLogs)
	}
	for _, f := range manifest.Files {
		data, ok := entries[f.Name]
		sum := sha256.Sum256(data)
		if !ok || int64(len(data)) != f.Bytes || hex.EncodeToString(sum[:]) != f.SHA256 {
			t.Fatal("manifest checksum mismatch")
		}
	}
}

func logsExportSources(t *testing.T) map[string]string {
	t.Helper()
	root, err := repoRoot()
	if err != nil {
		t.Fatal(err)
	}
	out, err := exec.Command("git", "-C", root, "ls-files", "--cached", "--others", "--exclude-standard", "--", "go.mod", "go.sum", "Makefile", "app/service/cmd/unic", "e2e/cli/logs_export_e2e_test.go").Output()
	if err != nil {
		t.Fatal(err)
	}
	result := map[string]string{}
	for _, name := range strings.Split(strings.TrimSpace(string(out)), "\n") {
		if name == "" {
			continue
		}
		data, err := os.ReadFile(filepath.Join(root, name))
		if os.IsNotExist(err) {
			continue
		}
		if err != nil {
			t.Fatal(err)
		}
		sum := sha256.Sum256(data)
		result[name] = hex.EncodeToString(sum[:])
	}
	return result
}

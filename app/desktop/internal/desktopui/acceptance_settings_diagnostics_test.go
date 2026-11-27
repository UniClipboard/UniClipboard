package desktopui

import (
	"archive/zip"
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"time"

	api "github.com/uniclipboard/unic/internal/localapi"
)

// The failure inventory precedes this scenario and runtime implementation in
// docs/exec/desktop-settings-diagnostics.md. No renderer or system clipboard runs.
func scenarioSettingsDiagnostics(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Minute)
	defer cancel()
	root, err := filepath.Abs("../../../..")
	if err != nil {
		t.Fatal(err)
	}
	private, err := os.MkdirTemp("/tmp", "udiag-")
	if err != nil {
		t.Fatal(err)
	}
	defer func() {
		if err := os.RemoveAll(private); err != nil {
			t.Error(err)
		}
	}()
	private, err = filepath.EvalSymlinks(private)
	if err != nil {
		t.Fatal(err)
	}
	checks := []string{}
	receipt := map[string]any{"native_ui": false, "renderer": false, "system_clipboard": false, "go": runtime.Version()}
	hash := func(b []byte) string { v := sha256.Sum256(b); return hex.EncodeToString(v[:]) }
	defer func() {
		receipt["passed"] = !t.Failed()
		receipt["checks"] = checks
		if out := os.Getenv("UNIC_DESKTOP_E2E_CASE_RECEIPT"); out != "" {
			b, err := json.MarshalIndent(receipt, "", "  ")
			if err != nil {
				t.Error(err)
				return
			}
			if err = os.MkdirAll(filepath.Dir(out), 0700); err != nil {
				t.Error(err)
				return
			}
			if err = os.WriteFile(out, append(b, '\n'), 0600); err != nil {
				t.Error(err)
			}
		}
	}()
	daemon, cli := filepath.Join(private, "unicd"), filepath.Join(private, "unic")
	for _, target := range []struct{ path, pkg string }{{daemon, "./app/service/cmd/unicd"}, {cli, "./app/service/cmd/unic"}} {
		c := exec.CommandContext(ctx, "go", "build", "-o", target.path, target.pkg)
		c.Dir = root
		if b, err := c.CombinedOutput(); err != nil {
			t.Fatalf("build: %v\n%s", err, b)
		}
		b, err := os.ReadFile(target.path)
		if err != nil {
			t.Fatal(err)
		}
		receipt[filepath.Base(target.path)+"_sha256"] = hash(b)
	}
	home := filepath.Join(private, "home")
	if err = os.MkdirAll(home, 0700); err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(home, "config.toml"), []byte("[telemetry]\nenabled=false\n[sync]\nenabled=false\n"), 0600); err != nil {
		t.Fatal(err)
	}
	var env []string
	for _, v := range os.Environ() {
		if !strings.HasPrefix(v, "UNIC_") {
			env = append(env, v)
		}
	}
	discovery := filepath.Join(private, "discovery")
	env = append(env, "UNIC_HOME="+home, "UNIC_RENDEZVOUS_URL=http://127.0.0.1:1",
		"HOME="+discovery, "USERPROFILE="+discovery, "APPDATA="+filepath.Join(discovery, "roaming"),
		"LOCALAPPDATA="+filepath.Join(discovery, "local"), "XDG_CONFIG_HOME="+filepath.Join(discovery, "config"),
		"XDG_DATA_HOME="+filepath.Join(discovery, "data"), "UC_PROFILE=")
	initialize := exec.CommandContext(ctx, cli, "migrate", "--initialize-empty", "--json")
	initialize.Env = env
	initialized, err := initialize.CombinedOutput()
	if err != nil {
		t.Fatalf("initialize private profile: %v %s", err, initialized)
	}
	checks = append(checks, "explicit production migration initialization with private source discovery")
	var proc *exec.Cmd
	var log bytes.Buffer
	start := func() {
		t.Helper()
		proc = exec.CommandContext(ctx, daemon)
		proc.Env = env
		proc.Stdout = &log
		proc.Stderr = &log
		if err := proc.Start(); err != nil {
			t.Fatal(err)
		}
	}
	stop := func() {
		if proc != nil {
			if err := proc.Process.Signal(os.Interrupt); err != nil {
				_ = proc.Process.Kill()
			}
			_ = proc.Wait()
			proc = nil
		}
	}
	defer stop()
	client := api.NewClient(filepath.Join(home, "unic.sock"))
	var screen *Desktop
	wait := func(label string, f func() bool) {
		t.Helper()
		deadline := time.Now().Add(20 * time.Second)
		for time.Now().Before(deadline) {
			if screen != nil {
				screen.d.live.poll(screen.d)
			}
			if f() {
				return
			}
			time.Sleep(15 * time.Millisecond)
		}
		t.Fatalf("timeout: %s", label)
	}
	start()
	wait("service", func() bool { _, err := client.Status(ctx); return err == nil })
	screen = NewDesktop(ctx, client)
	defer screen.Close()
	wait("loaded", func() bool { return screen.d.live.loaded })
	screen.d.refreshDiagnostics()
	wait("diagnostic status", func() bool { return screen.d.diagnostics.loaded && !screen.d.diagnostics.busy })
	if screen.d.diagnostics.err != "" {
		t.Fatal(screen.d.diagnostics.err)
	}
	if screen.d.diagnostics.telemetry.Effective {
		t.Fatal("telemetry unexpectedly active")
	}
	checks = append(checks, "read actual effective diagnostic state")
	cfg := screen.d.live.config.Config
	cfg.Logging.Level = "warn"
	cfg.Telemetry.Logs.Level = "error"
	cfg.Telemetry.Logs.Enabled = false
	cfg.Telemetry.Traces.Enabled = false
	cfg.Telemetry.Analytics.Enabled = false
	screen.d.saveLiveConfig(cfg)
	wait("saved logging", func() bool { return !screen.d.live.busy && screen.d.live.config.Config.Logging.Level == "warn" })
	screen.d.refreshDiagnostics()
	wait("effective logging", func() bool {
		return !screen.d.diagnostics.busy && screen.d.diagnostics.levels.Local.Configured == "warn"
	})
	if screen.d.diagnostics.levels.Local.Effective != "warn" || screen.d.diagnostics.levels.Remote.Configured != "error" {
		t.Fatal("saved/effective levels disagree")
	}
	checks = append(checks, "persistent local and remote settings apply")
	screen.d.setDiagnosticsOverride("local", false)
	wait("override", func() bool {
		return !screen.d.diagnostics.busy && screen.d.diagnostics.levels.Local.Source == "override"
	})
	if screen.d.diagnostics.levels.Local.Effective != "debug" || screen.d.diagnostics.levels.Local.Expires == nil {
		t.Fatal("override not bounded")
	}
	if screen.d.diagnostics.levels.Remote.Effective != "error" || screen.d.live.config.Config.Logging.Level != "warn" {
		t.Fatal("override changed unrelated saved/remote level")
	}
	screen.d.setDiagnosticsOverride("local", true)
	wait("override reset", func() bool { return !screen.d.diagnostics.busy && screen.d.diagnostics.levels.Local.Source == "config" })
	checks = append(checks, "bounded local override and reset preserve remote/saved levels")
	before := screen.d.live.config
	external := exec.CommandContext(ctx, cli, "config", "set", "device_name", "\"external edit\"")
	external.Env = env
	if b, err := external.CombinedOutput(); err != nil {
		t.Fatalf("external edit: %v %s", err, b)
	}
	screen.d.live.config = before
	cfg = before.Config
	cfg.Logging.Level = "debug"
	screen.d.saveLiveConfig(cfg)
	wait("revision refusal", func() bool { return !screen.d.live.busy })
	if screen.d.live.message == "" {
		t.Fatal("conflict was silent")
	}
	current, err := client.Config(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if current.Config.DeviceName != "external edit" || current.Config.Logging.Level != "warn" {
		t.Fatal("external edit overwritten")
	}
	checks = append(checks, "stale revision cannot overwrite external edit")
	screen.Close()
	screen = nil
	stop()
	start()
	wait("restart", func() bool { _, err := client.Status(ctx); return err == nil })
	persisted, err := client.Config(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if persisted.Config.Logging.Level != "warn" || persisted.Config.Telemetry.Logs.Enabled || persisted.Config.Telemetry.Traces.Enabled || persisted.Config.Telemetry.Analytics.Enabled {
		t.Fatal("settings lost after restart")
	}
	checks = append(checks, "diagnostic preferences survive service restart")
	logs := filepath.Join(home, "logs")
	if err = os.MkdirAll(logs, 0700); err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(logs, "segment-999999.jsonl"), []byte("{\"message\":\"synthetic-diagnostic\"}\n"), 0600); err != nil {
		t.Fatal(err)
	}
	output := filepath.Join(private, "diagnostics.zip")
	export := exec.CommandContext(ctx, cli, "dev", "logs", "export", "--output", output)
	export.Env = env
	if b, err := export.CombinedOutput(); err != nil {
		t.Fatalf("export: %v %s", err, b)
	}
	zr, err := zip.OpenReader(output)
	if err != nil {
		t.Fatal(err)
	}
	members := []string{}
	var manifestBytes []byte
	for _, f := range zr.File {
		if f.Name != "manifest.json" && !strings.HasPrefix(f.Name, "logs/engine/segment-") {
			t.Fatalf("unsafe archive member %s", f.Name)
		}
		if f.Name == "manifest.json" {
			rc, err := f.Open()
			if err != nil {
				t.Fatal(err)
			}
			manifestBytes, err = io.ReadAll(rc)
			_ = rc.Close()
			if err != nil {
				t.Fatal(err)
			}
		}
		members = append(members, f.Name)
	}
	if err = zr.Close(); err != nil {
		t.Fatal(err)
	}
	if len(members) < 2 || len(manifestBytes) == 0 {
		t.Fatal("archive missing log/manifest")
	}
	var manifest struct {
		FormatVersion   int    `json:"format_version"`
		ServiceSnapshot string `json:"service_snapshot"`
	}
	if err = json.Unmarshal(manifestBytes, &manifest); err != nil || manifest.FormatVersion != 1 || manifest.ServiceSnapshot != "live" {
		t.Fatalf("unexpected diagnostic manifest: %s", manifestBytes)
	}
	original, err := os.ReadFile(output)
	if err != nil {
		t.Fatal(err)
	}
	export = exec.CommandContext(ctx, cli, "dev", "logs", "export", "--output", output)
	export.Env = env
	if err = export.Run(); err == nil {
		t.Fatal("export replaced existing destination")
	}
	after, err := os.ReadFile(output)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(original, after) {
		t.Fatal("existing archive changed")
	}
	receipt["archive_members"] = members
	receipt["archive_sha256"] = hash(original)
	checks = append(checks, "production CLI export is bounded, allowlisted and refuses overwrite")
}

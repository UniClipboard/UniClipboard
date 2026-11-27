package desktopui

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"time"

	api "github.com/uniclipboard/unic/internal/localapi"
)

// The failure inventory and portable allowlist precede runtime implementation.
// This crosses the production Desktop binding and daemon; no renderer runs.
func scenarioSettingsBackup(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Minute)
	defer cancel()
	root, err := filepath.Abs("../../../..")
	if err != nil {
		t.Fatal(err)
	}
	private, err := os.MkdirTemp("/tmp", "uback-")
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

	cfg := screen.d.live.config.Config
	cfg.Logging.Level = "warn"
	cfg.History.RetentionDays = 90
	cfg.Desktop.Language = "en"
	cfg.Sync.Relay.Enabled = true
	cfg.Sync.Relay.URL = "https://relay.private.example"
	cfg.Sync.Relay.AccessKey = "backup-must-never-include-relay-key"
	encoded, err := encodePortableSettings(cfg)
	if err != nil {
		t.Fatal(err)
	}
	for _, excluded := range []string{"device_name", "inbox", "rendezvous_url", "relay", "access_key", "backup-must-never-include-relay-key", "endpoint", "plugins", "launch_at_login", "quick_panel_shortcut", "excluded_tags", "tag_colors"} {
		if bytes.Contains(encoded, []byte(excluded)) {
			t.Fatalf("excluded field in backup: %s", excluded)
		}
	}
	checks = append(checks, "explicit export allowlist excludes machine paths and private settings")
	before, err := os.ReadFile(filepath.Join(home, "config.toml"))
	if err != nil {
		t.Fatal(err)
	}
	screen.d.prepareBackupImport(encoded, screen.d.live.config)
	if screen.d.backup.preview == nil || len(screen.d.backup.preview.changes) < 3 {
		t.Fatal("missing readable preview")
	}
	still, err := os.ReadFile(filepath.Join(home, "config.toml"))
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(before, still) {
		t.Fatal("preview wrote configuration")
	}
	screen.d.cancelBackupImport()
	if screen.d.backup.preview != nil {
		t.Fatal("cancel retained import")
	}
	checks = append(checks, "preview and cancellation never mutate configuration")
	invalid := [][]byte{[]byte("{"), append(append([]byte{}, encoded...), []byte("{}")...), bytes.Replace(encoded, []byte(`"version": 1`), []byte(`"version": 2`), 1), bytes.Replace(encoded, []byte(`"logging.level": "warn"`), []byte(`"logging.level": "invalid"`), 1), []byte(strings.Repeat(" ", MaxSettingsBackupBytes+1)), bytes.Replace(encoded, []byte(`"logging.level": "warn"`), []byte(`"plugin.secret": "sensitive"`), 1)}
	for i, data := range invalid {
		screen.d.prepareBackupImport(data, screen.d.live.config)
		if screen.d.backup.preview != nil || screen.d.backup.err == "" {
			t.Fatalf("invalid input %d accepted", i)
		}
		actual, e := os.ReadFile(filepath.Join(home, "config.toml"))
		if e != nil || !bytes.Equal(actual, before) {
			t.Fatalf("invalid input %d mutated config", i)
		}
	}
	checks = append(checks, "malformed oversize unknown version invalid value unknown key and trailing JSON rejected")
	screen.d.prepareBackupImport(encoded, screen.d.live.config)
	if screen.d.backup.preview == nil {
		t.Fatal("valid import not prepared")
	}
	screen.d.confirmBackupImport()
	wait("import saved", func() bool { return !screen.d.live.busy && screen.d.live.config.Config.Logging.Level == "warn" })
	if screen.d.live.config.Config.Desktop.Language != "en" || screen.d.live.config.Config.History.RetentionDays != 90 {
		t.Fatal("portable values not applied")
	}
	if screen.d.live.config.Config.DeviceName != cfg.DeviceName || screen.d.live.config.Config.Inbox != cfg.Inbox || screen.d.live.config.Config.RendezvousURL != cfg.RendezvousURL {
		t.Fatal("excluded local state changed")
	}
	checks = append(checks, "confirmed import applies through revision-checked production config and preserves excluded state")
	saved := screen.d.live.config
	changed := saved.Config
	changed.Logging.Level = "error"
	updated, err := encodePortableSettings(changed)
	if err != nil {
		t.Fatal(err)
	}
	screen.d.prepareBackupImport(updated, saved)
	external := exec.CommandContext(ctx, cli, "config", "set", "device_name", "\"external backup edit\"")
	external.Env = env
	if b, err := external.CombinedOutput(); err != nil {
		t.Fatalf("external edit: %v %s", err, b)
	}
	screen.d.confirmBackupImport()
	wait("conflict refused", func() bool { return !screen.d.live.busy })
	current, err := client.Config(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if current.Config.DeviceName != "external backup edit" || current.Config.Logging.Level != "warn" {
		t.Fatal("stale import overwrote external changes")
	}
	if screen.d.backup.err == "" && screen.d.live.message == "" {
		t.Fatal("conflict not visible")
	}
	checks = append(checks, "confirmation is bound to preview revision and cannot overwrite external changes")
	screen.Close()
	screen = nil
	stop()
	start()
	wait("restart", func() bool { _, err := client.Status(ctx); return err == nil })
	final, err := client.Config(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if final.Config.Logging.Level != "warn" || final.Config.Desktop.Language != "en" || final.Config.History.RetentionDays != 90 || final.Config.DeviceName != "external backup edit" {
		t.Fatal("confirmed portable preferences not preserved on restart")
	}
	checks = append(checks, "portable import and excluded local changes survive restart")
	receipt["export_sha256"] = hash(encoded)
}

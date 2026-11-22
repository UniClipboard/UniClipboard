package desktopui

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

// Failure inventory, written before runtime implementation: absent default;
// accepted invalid mode; stale writes overwrite external edits; appearance
// loses unrelated settings; watcher omits changes; restart loses saved mode.
// This real-process case never constructs a renderer or native Desktop window.
func scenarioSettingsAppearance(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Minute)
	defer cancel()
	root, err := filepath.Abs("../../../..")
	if err != nil {
		t.Fatal(err)
	}
	private, err := os.MkdirTemp("/tmp", "unic-appearance-")
	if err != nil {
		t.Fatal(err)
	}
	private, err = filepath.EvalSymlinks(private)
	if err != nil {
		t.Fatal(err)
	}
	checks := []string{}
	receipt := map[string]any{"native_ui": false, "renderer": false, "system_clipboard": false}
	defer func() {
		cleanupErr := os.RemoveAll(private)
		if cleanupErr != nil {
			t.Error(cleanupErr)
		}
		receipt["cleanup_passed"] = cleanupErr == nil
		receipt["checks"] = checks
		receipt["passed"] = !t.Failed()
		if out := os.Getenv("UNIC_DESKTOP_E2E_CASE_RECEIPT"); out != "" {
			raw, marshalErr := json.MarshalIndent(receipt, "", "  ")
			if marshalErr != nil {
				t.Error(marshalErr)
				return
			}
			if e := os.MkdirAll(filepath.Dir(out), 0700); e != nil {
				t.Error(e)
				return
			}
			if e := os.WriteFile(out, raw, 0600); e != nil {
				t.Error(e)
			}
		}
	}()
	exe := filepath.Join(private, "unicd")
	build := exec.CommandContext(ctx, "go", "build", "-race", "-o", exe, "./app/service/cmd/unicd")
	build.Dir = root
	if out, e := build.CombinedOutput(); e != nil {
		t.Fatalf("build service: %v\n%s", e, out)
	}
	binary, err := os.ReadFile(exe)
	if err != nil {
		t.Fatal(err)
	}
	digest := sha256.Sum256(binary)
	receipt["unicd_sha256"] = hex.EncodeToString(digest[:])
	home := filepath.Join(private, "home")
	seed, err := engine.New(ctx, engine.Options{DataDir: home, MigrationMode: true})
	if err != nil {
		t.Fatal(err)
	}
	if err := seed.Close(ctx); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(home, "config.toml")
	document := func(mode string) []byte {
		appearance := ""
		if mode != "" {
			appearance = fmt.Sprintf("[desktop]\ncolor_mode=%q\n", mode)
		}
		return []byte("device_name=\"Appearance acceptance\"\n[telemetry]\nenabled=false\n[sync]\nenabled=false\n" + appearance)
	}
	if err := os.WriteFile(path, document(""), 0600); err != nil {
		t.Fatal(err)
	}
	env := []string{}
	for _, value := range os.Environ() {
		if !strings.HasPrefix(value, "UNIC_") {
			env = append(env, value)
		}
	}
	var proc *exec.Cmd
	start := func() {
		t.Helper()
		proc = exec.CommandContext(ctx, exe)
		proc.Env = append(env, "UNIC_HOME="+home, "UNIC_RENDEZVOUS_URL=http://127.0.0.1:1")
		if err := proc.Start(); err != nil {
			t.Fatal(err)
		}
	}
	stop := func() {
		t.Helper()
		if proc == nil {
			return
		}
		if err := proc.Process.Signal(os.Interrupt); err != nil {
			t.Error(err)
		}
		if err := proc.Wait(); err != nil {
			t.Errorf("owned service exit: %v", err)
		}
		proc = nil
	}
	defer stop()
	client := api.NewClient(filepath.Join(home, "unic.sock"))
	wait := func(label string, check func() bool) {
		t.Helper()
		until := time.Now().Add(15 * time.Second)
		for time.Now().Before(until) {
			if check() {
				return
			}
			time.Sleep(20 * time.Millisecond)
		}
		t.Fatalf("timeout: %s", label)
	}
	read := func() config.State {
		t.Helper()
		state, err := client.Config(ctx)
		if err != nil {
			t.Fatal(err)
		}
		if state.Config.Sync.Enabled || state.Config.Telemetry.Enabled || state.Config.DeviceName != "Appearance acceptance" {
			t.Fatal("appearance write changed unrelated configuration")
		}
		return state
	}
	mode := func(state config.State) string {
		t.Helper()
		raw, err := json.Marshal(state.Config.Desktop)
		if err != nil {
			t.Fatal(err)
		}
		var desktop map[string]any
		if err := json.Unmarshal(raw, &desktop); err != nil {
			t.Fatal(err)
		}
		value, _ := desktop["color_mode"].(string)
		return value
	}
	project := func(state config.State, expected string) {
		t.Helper()
		d := &viewState{live: &liveState{config: state}, options: map[string]bool{}, choices: map[string]string{}}
		d.projectLiveConfig()
		if d.theme != expected {
			t.Fatalf("production config projection: got %q, want %q", d.theme, expected)
		}
	}
	start()
	wait("initial service", func() bool { _, err := client.Status(ctx); return err == nil })
	state := read()
	if got := mode(state); got != "system" {
		t.Fatalf("default color mode: got %q, want system", got)
	}
	project(state, "system")
	checks = append(checks, "color mode defaults to system without touching other settings")
	for _, value := range []string{"dark", "light", "system"} {
		state, err = client.SaveConfig(ctx, state.Revision, document(value))
		if err != nil || mode(state) != value {
			t.Fatalf("save %s: %v", value, err)
		}
		if got := mode(read()); got != value {
			t.Fatalf("read after save %s: got %q", value, got)
		}
		project(state, value)
	}
	checks = append(checks, "all supported modes save and read through the production service")
	stale := state.Revision
	state, err = client.SaveConfig(ctx, state.Revision, document("dark"))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := client.SaveConfig(ctx, stale, document("light")); err == nil {
		t.Fatal("stale revision was accepted")
	}
	if got := mode(read()); got != "dark" {
		t.Fatal("stale write changed effective mode")
	}
	if _, err := client.SaveConfig(ctx, state.Revision, document("unsupported")); err == nil {
		t.Fatal("invalid color mode was accepted")
	}
	if got := mode(read()); got != "dark" {
		t.Fatal("invalid write changed effective mode")
	}
	checks = append(checks, "invalid and stale writes preserve the effective mode")
	if err := os.WriteFile(path, document("light"), 0600); err != nil {
		t.Fatal(err)
	}
	wait("external config reload", func() bool {
		state, err := client.Config(ctx)
		return err == nil && mode(state) == "light"
	})
	checks = append(checks, "external file changes publish the saved mode")
	if err := os.WriteFile(path, document("unsupported"), 0600); err != nil {
		t.Fatal(err)
	}
	wait("invalid external config rejection", func() bool {
		state, err := client.Config(ctx)
		return err == nil && state.Error != "" && mode(state) == "light"
	})
	project(read(), "light")
	if err := os.WriteFile(path, document("light"), 0600); err != nil {
		t.Fatal(err)
	}
	wait("valid external config recovery", func() bool {
		state, err := client.Config(ctx)
		return err == nil && state.Error == "" && mode(state) == "light"
	})
	checks = append(checks, "invalid external edits retain the last valid mode and valid edits recover")
	stop()
	start()
	wait("restarted service", func() bool { _, err := client.Status(ctx); return err == nil })
	if got := mode(read()); got != "light" {
		t.Fatalf("restart lost saved color mode: %q", got)
	}
	project(read(), "light")
	checks = append(checks, "service restart preserves saved appearance and unrelated settings")
	stop()
}

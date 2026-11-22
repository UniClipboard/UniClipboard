package desktopui

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
)

// Written before the permission UI and host implementation. Failures: passive
// reads request permission; settings opening implies a grant; blocked reads
// block guidance; unsupported/error states become granted; stale callback or
// cancelled completions overwrite current state. Native permission calls are
// substituted only at the Desktop host boundary, never by granting OS access.
func scenarioSettingsShortcuts(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Minute)
	defer cancel()
	root, err := filepath.Abs("../../../..")
	if err != nil {
		t.Fatal(err)
	}
	private, err := os.MkdirTemp("/tmp", "unic-permissions-")
	if err != nil {
		t.Fatal(err)
	}
	private, err = filepath.EvalSymlinks(private)
	if err != nil {
		t.Fatal(err)
	}
	checks := []string{}
	receipt := map[string]any{"native_ui": false, "renderer": false, "native_permission": false, "system_clipboard": false}
	defer func() {
		cleanupErr := os.RemoveAll(private)
		if cleanupErr != nil {
			t.Error(cleanupErr)
		}
		receipt["cleanup_passed"] = cleanupErr == nil
		receipt["passed"] = !t.Failed()
		receipt["checks"] = checks
		if out := os.Getenv("UNIC_DESKTOP_E2E_CASE_RECEIPT"); out != "" {
			raw, err := json.MarshalIndent(receipt, "", "  ")
			if err != nil {
				t.Error(err)
				return
			}
			if err := os.MkdirAll(filepath.Dir(out), 0700); err != nil {
				t.Error(err)
				return
			}
			if err := os.WriteFile(out, raw, 0600); err != nil {
				t.Error(err)
			}
		}
	}()
	exe := filepath.Join(private, "unicd")
	build := exec.CommandContext(ctx, "go", "build", "-race", "-o", exe, "./app/service/cmd/unicd")
	build.Dir = root
	if out, err := build.CombinedOutput(); err != nil {
		t.Fatalf("build service: %v\n%s", err, out)
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
	if err := os.WriteFile(filepath.Join(home, "config.toml"), []byte("[telemetry]\nenabled=false\n[sync]\nenabled=false\n"), 0600); err != nil {
		t.Fatal(err)
	}
	var env []string
	for _, value := range os.Environ() {
		if !strings.HasPrefix(value, "UNIC_") {
			env = append(env, value)
		}
	}
	proc := exec.CommandContext(ctx, exe)
	proc.Env = append(env, "UNIC_HOME="+home, "UNIC_RENDEZVOUS_URL=http://127.0.0.1:1")
	if err := proc.Start(); err != nil {
		t.Fatal(err)
	}
	defer func() {
		if err := proc.Process.Signal(os.Interrupt); err != nil {
			t.Error(err)
		}
		if err := proc.Wait(); err != nil {
			t.Error(err)
		}
	}()
	client := api.NewClient(filepath.Join(home, "unic.sock"))
	var screen *Desktop
	wait := func(label string, check func() bool) {
		t.Helper()
		until := time.Now().Add(15 * time.Second)
		for time.Now().Before(until) {
			if screen != nil {
				screen.d.live.poll(screen.d)
			}
			if check() {
				return
			}
			time.Sleep(10 * time.Millisecond)
		}
		t.Fatalf("timeout: %s", label)
	}
	wait("service", func() bool { _, err := client.Status(ctx); return err == nil })
	screen = NewDesktop(ctx, client)
	defer screen.Close()
	wait("production binding", func() bool { return screen.d.live.loaded && !screen.d.live.loading })
	d := screen.d
	var opens atomic.Int32
	open := func(context.Context) error { opens.Add(1); return nil }
	for _, status := range []string{"denied", "granted", "unknown", "unsupported"} {
		screen.SetShortcutPermissionActions(func(context.Context) (string, error) { return status, nil }, open)
		d.refreshShortcutPermission()
		wait("permission read "+status, func() bool { return !d.shortcuts.loading })
		if d.shortcuts.status != status || d.shortcuts.err != "" || opens.Load() != 0 {
			t.Fatalf("passive status %s incorrectly opened settings or changed state", status)
		}
	}
	checks = append(checks, "passive reads preserve distinct denied/granted/unknown/unsupported states without settings actions")
	screen.SetShortcutPermissionActions(func(context.Context) (string, error) { return "denied", nil }, open)
	d.refreshShortcutPermission()
	wait("denied before guidance", func() bool { return !d.shortcuts.loading })
	d.openShortcutPermissionSettings()
	wait("explicit guidance", func() bool { return !d.shortcuts.opening })
	if opens.Load() != 1 || d.shortcuts.status != "denied" || d.shortcuts.notice == "" {
		t.Fatal("settings guidance was counted as permission grant")
	}
	checks = append(checks, "only explicit guidance opens settings and it never grants permission")
	screen.SetShortcutPermissionActions(func(context.Context) (string, error) { return "granted", errors.New("private native error") }, open)
	d.refreshShortcutPermission()
	wait("read failure", func() bool { return !d.shortcuts.loading })
	if d.shortcuts.status != "unknown" || d.shortcuts.err == "" || strings.Contains(d.shortcuts.err, "private") {
		t.Fatal("failed read became granted or leaked a raw native error")
	}
	checks = append(checks, "failed native reads become sanitized unknown state")
	started, release, completed := make(chan struct{}), make(chan struct{}), make(chan struct{})
	screen.SetShortcutPermissionActions(func(context.Context) (string, error) {
		close(started)
		<-release
		defer close(completed)
		return "denied", nil
	}, open)
	d.refreshShortcutPermission()
	select {
	case <-started:
	case <-ctx.Done():
		t.Fatal(ctx.Err())
	}
	d.openShortcutPermissionSettings()
	wait("guidance overlaps read", func() bool { return !d.shortcuts.opening })
	if !d.shortcuts.loading || opens.Load() != 2 {
		t.Fatal("blocked status read prevented independent settings guidance")
	}
	screen.SetShortcutPermissionActions(func(context.Context) (string, error) { return "granted", nil }, open)
	d.refreshShortcutPermission()
	wait("replacement permission result", func() bool { return !d.shortcuts.loading && d.shortcuts.status == "granted" })
	close(release)
	<-completed
	// The old callback completed before returning its result. Drain all returned
	// work while waiting for the production queue to settle, then inspect state.
	until := time.Now().Add(100 * time.Millisecond)
	for time.Now().Before(until) {
		d.live.poll(d)
		time.Sleep(5 * time.Millisecond)
	}
	if d.shortcuts.status != "granted" {
		t.Fatal("a replaced callback overwrote the newer permission state")
	}
	checks = append(checks, "settings guidance does not block on reads and replaced callbacks cannot overwrite newer state")
	late := make(chan struct{})
	lateDone := make(chan struct{})
	screen.SetShortcutPermissionActions(func(context.Context) (string, error) { return "denied", nil }, func(context.Context) error {
		<-late
		defer close(lateDone)
		return nil
	})
	d.openShortcutPermissionSettings()
	screen.Close()
	close(late)
	<-lateDone
	until = time.Now().Add(100 * time.Millisecond)
	for time.Now().Before(until) {
		d.live.poll(d)
		time.Sleep(5 * time.Millisecond)
	}
	if d.shortcuts.notice != "" {
		t.Fatal("cancelled settings action published a late success")
	}
	checks = append(checks, "cancelled Desktop actions cannot publish late settings-open success")
}

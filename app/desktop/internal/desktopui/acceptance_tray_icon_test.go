package desktopui

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"image/png"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"slices"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	proof "github.com/uniclipboard/unic/app/desktop/internal/e2eproof"
	"github.com/uniclipboard/unic/app/desktop/internal/trayicon"
	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

// Failure inventory and exact boundaries precede implementation in
// docs/exec/desktop-tray-icon-styles.md. This is a real-service view E2E;
// the image sink observes the production native API boundary without a GUI.
func scenarioTrayIconSettings(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Minute)
	defer cancel()
	root, err := filepath.Abs("../../../..")
	if err != nil {
		t.Fatal(err)
	}
	private, err := os.MkdirTemp("", "unic-tray-e2e-")
	if err != nil {
		t.Fatal(err)
	}
	defer func() {
		if err := os.RemoveAll(private); err != nil {
			t.Error(err)
		}
	}()
	bin := filepath.Join(private, "bin")
	home := filepath.Join(private, "home")
	for _, path := range []string{bin, home} {
		if err := os.MkdirAll(path, 0700); err != nil {
			t.Fatal(err)
		}
	}
	suffix := ""
	if runtime.GOOS == "windows" {
		suffix = ".exe"
	}
	for _, name := range []string{"unic", "unicd"} {
		cmd := exec.CommandContext(ctx, "go", "build", "-o", filepath.Join(bin, name+suffix), "./app/service/cmd/"+name)
		cmd.Dir = root
		if out, err := cmd.CombinedOutput(); err != nil {
			t.Fatalf("build %s: %v\n%s", name, err, out)
		}
	}
	path := filepath.Join(home, "config.toml")
	initial := []byte("[telemetry]\nenabled=false\n[sync]\nenabled=false\n")
	if err := os.WriteFile(path, initial, 0600); err != nil {
		t.Fatal(err)
	}
	env := []string{}
	for _, entry := range os.Environ() {
		if !strings.HasPrefix(entry, "UNIC_") {
			env = append(env, entry)
		}
	}
	discovery := filepath.Join(private, "discovery")
	env = append(env, "UNIC_HOME="+home, "UNIC_RENDEZVOUS_URL=http://127.0.0.1:1",
		"HOME="+discovery, "USERPROFILE="+discovery, "APPDATA="+filepath.Join(discovery, "roaming"),
		"LOCALAPPDATA="+filepath.Join(discovery, "local"), "XDG_CONFIG_HOME="+filepath.Join(discovery, "config"),
		"XDG_DATA_HOME="+filepath.Join(discovery, "data"), "UC_PROFILE=")
	initialize := exec.CommandContext(ctx, filepath.Join(bin, "unic"+suffix), "migrate", "--initialize-empty", "--json")
	initialize.Env = env
	output, err := initialize.CombinedOutput()
	var initialized map[string]any
	if err != nil || json.Unmarshal(output, &initialized) != nil || initialized["status"] != "complete" || initialized["outcome"] != "no_source" {
		t.Fatalf("initialize tray fixture before service start: %v %s", err, output)
	}
	log, err := os.Create(filepath.Join(private, "unicd.log"))
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = log.Close() }()
	cmd := exec.CommandContext(ctx, filepath.Join(bin, "unicd"+suffix))
	cmd.Env, cmd.Stdout, cmd.Stderr = env, log, log
	if err := cmd.Start(); err != nil {
		t.Fatal(err)
	}
	defer func() { _ = cmd.Process.Kill(); _ = cmd.Wait() }()
	client := api.NewClient(filepath.Join(home, "unic.sock"))
	wait := func(label string, condition func() bool) {
		t.Helper()
		deadline := time.Now().Add(20 * time.Second)
		for time.Now().Before(deadline) {
			if condition() {
				return
			}
			time.Sleep(20 * time.Millisecond)
		}
		t.Fatal("timeout: " + label)
	}
	wait("service", func() bool { _, err := client.Status(ctx); return err == nil })
	manager, err := config.Open(path, nil)
	if err != nil {
		t.Fatal(err)
	}
	changes := make(chan config.Config, 32)
	manager.OnChange = func(_, current config.Config, err error) {
		if err == nil {
			changes <- current
		}
	}
	watchCtx, stopWatch := context.WithCancel(ctx)
	watchDone := make(chan error, 1)
	go func() { watchDone <- manager.Watch(watchCtx) }()
	defer func() {
		stopWatch()
		if err := <-watchDone; err != nil {
			t.Errorf("stop configuration watcher: %v", err)
		}
	}()
	images := []string{}
	failImage := false
	icon := trayicon.New(func(raw []byte, template bool) error {
		if failImage {
			return errors.New("synthetic native image failure")
		}
		img, err := png.Decode(bytes.NewReader(raw))
		if err != nil {
			return err
		}
		if img.Bounds().Dx() < 16 {
			return errors.New("invalid tray dimensions")
		}
		if template != (runtime.GOOS != "linux") {
			return errors.New("wrong template mode")
		}
		if len(images) == 0 {
			_, _, _, alpha := img.At(img.Bounds().Dx()/2, img.Bounds().Dy()/2).RGBA()
			if alpha != 0 {
				return errors.New("glyph interior is not transparent")
			}
		}
		sum := sha256.Sum256(raw)
		images = append(images, hex.EncodeToString(sum[:]))
		artifactDir := os.Getenv("UNIC_DESKTOP_E2E_CASE_ARTIFACTS")
		if err := os.WriteFile(filepath.Join(artifactDir, "tray-image-"+hex.EncodeToString(sum[:])+".png"), raw, 0600); err != nil {
			return err
		}
		return nil
	})
	if manager.Current().Desktop.TrayIcon != "glyph" {
		t.Fatal("default must match v1 glyph")
	}
	if err := icon.SetStyle(manager.Current().Desktop.TrayIcon); err != nil {
		t.Fatal(err)
	}
	glyphHash := images[0]
	desktop := NewDesktop(ctx, client)
	defer desktop.Close()
	d := desktop.d
	d.page, d.settings.Value = "settings", "general"
	wait("settings", func() bool { d.live.poll(d); return d.live.loaded })
	checks := []string{"omitted desktop.tray_icon defaults to glyph"}
	// Establish that the real directory watcher is ready before mutating preferences.
	deadline := time.Now().Add(10 * time.Second)
	for len(changes) == 0 && time.Now().Before(deadline) {
		if err := os.WriteFile(path, initial, 0600); err != nil {
			t.Fatal(err)
		}
		time.Sleep(250 * time.Millisecond)
	}
	if len(changes) == 0 {
		t.Fatal("watcher did not start")
	}
	drain := func() {
		for len(changes) > 0 {
			cfg := <-changes
			if err := icon.SetStyle(cfg.Desktop.TrayIcon); err != nil {
				t.Fatal(err)
			}
		}
	}
	drain()
	t.Setenv("UNIC_UI_SKIN", "macos")
	view := ui.NewTesterWith(d.view, 1100, 950, ui.TesterOptions{})
	proof.Capture(t, view)
	choose := func(value, label string) {
		t.Helper()
		wait("selector idle", func() bool { d.live.poll(d); view.Advance(20 * time.Millisecond); return !d.live.busy })
		view.Advance(500 * time.Millisecond) // Keep consecutive choices distinct from a double click.
		target, ok := view.AccessByRole(ui.RolePopUpButton, "托盘图标")
		if !ok {
			t.Fatal("tray style selector missing")
		}
		x, y := target.Bounds.X+target.Bounds.W/2, target.Bounds.Y+target.Bounds.H/2
		view.Press(x, y)
		view.Release(x, y)
		if err := view.ChooseMenuItem(label); err != nil {
			t.Fatal(err)
		}
		wait("saved and applied "+value, func() bool {
			d.live.poll(d)
			view.Advance(20 * time.Millisecond)
			drain()
			return !d.live.busy && d.live.config.Config.Desktop.TrayIcon == value && manager.Current().Desktop.TrayIcon == value
		})
		state, err := client.Config(ctx)
		if err != nil || state.Config.Desktop.TrayIcon != value {
			t.Fatal("service preference mismatch", err)
		}
		if slices.Contains(state.RestartRequired, "desktop.tray_icon") {
			t.Fatal("style unexpectedly requires restart")
		}
		raw, err := os.ReadFile(path)
		if err != nil {
			t.Fatal(err)
		}
		cfg, err := config.Parse(raw)
		if err != nil || cfg.Desktop.TrayIcon != value {
			t.Fatal("style not persisted", err)
		}
	}
	choose("cat", "经典猫头")
	catHash := images[len(images)-1]
	if catHash == glyphHash {
		t.Fatal("different styles delivered the same PNG")
	}
	checks = append(checks, "settings cat choice persists and reaches the image sink without restart")
	choose("glyph", "默认")
	if images[len(images)-1] != glyphHash {
		t.Fatal("glyph round trip changed image")
	}
	choose("cat", "经典猫头")
	count := len(images)
	if err := icon.SetStyle("cat"); err != nil {
		t.Fatal(err)
	}
	if len(images) != count {
		t.Fatal("unchanged style republished image")
	}
	if err := icon.Refresh(); err != nil {
		t.Fatal(err)
	}
	if images[len(images)-1] != catHash {
		t.Fatal("theme repaint reset style")
	}
	checks = append(checks, "both switch directions, unchanged-style suppression and repaint preserve the selected image")
	reopened := NewDesktop(ctx, client)
	defer reopened.Close()
	wait("reopened style", func() bool { reopened.d.live.poll(reopened.d); return reopened.d.live.loaded })
	if reopened.d.live.config.Config.Desktop.TrayIcon != "cat" {
		t.Fatal("style lost on reopen")
	}
	reopenedManager, err := config.Open(path, nil)
	if err != nil || reopenedManager.Current().Desktop.TrayIcon != "cat" {
		t.Fatal("style lost on manager restart", err)
	}
	checks = append(checks, "reopened Desktop and restarted configuration owner retain cat")
	cli := func(value string, success bool) {
		t.Helper()
		command := exec.CommandContext(ctx, filepath.Join(bin, "unic"+suffix), "config", "set", "desktop.tray_icon", strconv.Quote(value))
		command.Env = env
		output, err := command.CombinedOutput()
		if (err == nil) != success {
			t.Fatalf("CLI style %s: %v %s", value, err, output)
		}
	}
	cli("glyph", true)
	wait("CLI image delivery", func() bool {
		drain()
		return manager.Current().Desktop.TrayIcon == "glyph" && images[len(images)-1] == glyphHash
	})
	cli("unknown", false)
	state, err := client.Config(ctx)
	if err != nil || state.Config.Desktop.TrayIcon != "glyph" {
		t.Fatal("invalid CLI replaced configuration", err)
	}
	count = len(images)
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	invalid := strings.Replace(string(raw), `tray_icon = "glyph"`, `tray_icon = "invalid"`, 1)
	if invalid == string(raw) {
		t.Fatal("fixture did not change tray_icon")
	}
	if err := os.WriteFile(path, []byte(invalid), 0600); err != nil {
		t.Fatal(err)
	}
	wait("invalid file rejection", func() bool { return manager.State().Error != "" })
	drain()
	if len(images) != count || manager.Current().Desktop.TrayIcon != "glyph" {
		t.Fatal("invalid file replaced current image")
	}
	valid := strings.Replace(string(raw), `tray_icon = "glyph"`, `tray_icon = "cat"`, 1)
	if err := os.WriteFile(path, []byte(valid), 0600); err != nil {
		t.Fatal(err)
	}
	wait("valid file recovery", func() bool {
		drain()
		return manager.Current().Desktop.TrayIcon == "cat" && images[len(images)-1] == catHash
	})
	checks = append(checks, "real CLI and file edits reach the image owner; invalid values retain the last valid configuration")
	count = len(images)
	failImage = true
	if err := icon.SetStyle("glyph"); err == nil {
		t.Fatal("failed image delivery reported success")
	}
	failImage = false
	if err := icon.SetStyle("glyph"); err != nil {
		t.Fatal(err)
	}
	if len(images) != count+1 || images[len(images)-1] != glyphHash {
		t.Fatal("failed image was not retried")
	}
	checks = append(checks, "failed native image delivery leaves the old style retryable")
	receipt := map[string]any{"status": "passed", "checks": checks, "glyph_sha256": glyphHash, "cat_sha256": catHash, "published_images": images, "native_tray": false, "native_window": false, "system_clipboard": false}
	out := os.Getenv("UNIC_DESKTOP_E2E_CASE_RECEIPT")
	if out == "" {
		t.Fatal("canonical runner receipt path missing")
	}
	data, err := json.MarshalIndent(receipt, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(out, data, 0600); err != nil {
		t.Fatal(err)
	}
}

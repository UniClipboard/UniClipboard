package desktopui

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
)

// Failure inventory: ledger inventory confused with disk usage; custom files
// counted or deleted; failed refresh reported as fresh; duplicate reads queued;
// native callback failure leaks private paths; reading mutates configuration.
// This scenario is specified before its production settings implementation.
func scenarioSettingsStorageProcess(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Minute)
	defer cancel()
	root, err := filepath.Abs("../../../..")
	if err != nil {
		t.Fatal(err)
	}
	private, err := os.MkdirTemp("/tmp", "unic-storage-settings-")
	if err != nil {
		t.Fatal(err)
	}
	private, err = filepath.EvalSymlinks(private)
	if err != nil {
		t.Fatal(err)
	}
	checks := []string{}
	receipt := map[string]any{"native_ui": false, "renderer": false, "system_clipboard": false, "file_manager": false}
	defer func() {
		receipt["cleanup_passed"] = os.RemoveAll(private) == nil
		if receipt["cleanup_passed"] != true {
			t.Error("private state cleanup failed")
		}
		receipt["passed"] = !t.Failed()
		receipt["checks"] = checks
		if p := os.Getenv("UNIC_DESKTOP_E2E_CASE_RECEIPT"); p != "" {
			b, e := json.MarshalIndent(receipt, "", "  ")
			if e != nil {
				t.Error(e)
				return
			}
			if e = os.MkdirAll(filepath.Dir(p), 0700); e != nil {
				t.Error(e)
				return
			}
			if e = os.WriteFile(p, b, 0600); e != nil {
				t.Error(e)
			}
		}
	}()
	exe := filepath.Join(private, "unicd")
	build := exec.CommandContext(ctx, "go", "build", "-race", "-o", exe, "./app/service/cmd/unicd")
	build.Dir = root
	if out, e := build.CombinedOutput(); e != nil {
		t.Fatalf("production daemon build: %v\n%s", e, out)
	}
	binary, err := os.ReadFile(exe)
	if err != nil {
		t.Fatal(err)
	}
	hash := sha256.Sum256(binary)
	receipt["unicd_sha256"] = hex.EncodeToString(hash[:])
	home := filepath.Join(private, "home")
	initial, err := engine.New(ctx, engine.Options{DataDir: home, MigrationMode: true})
	if err != nil {
		t.Fatal(err)
	}
	if err = initial.Close(ctx); err != nil {
		t.Fatal(err)
	}
	unknown := filepath.Join(home, "inbox", "file-cache", "user-owned-unknown")
	if err = os.MkdirAll(filepath.Dir(unknown), 0700); err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(unknown, []byte("not a ledger payload"), 0600); err != nil {
		t.Fatal(err)
	}
	custom := filepath.Join(private, "custom-inbox")
	if err = os.MkdirAll(custom, 0700); err != nil {
		t.Fatal(err)
	}
	customFile := filepath.Join(custom, "user-original")
	if err = os.WriteFile(customFile, []byte("preserve custom bytes"), 0600); err != nil {
		t.Fatal(err)
	}
	configPath := filepath.Join(home, "config.toml")
	configText := "[telemetry]\nenabled=false\n[sync]\nenabled=false\n[history]\nauto_cleanup=false\n"
	if err = os.WriteFile(configPath, []byte(configText), 0600); err != nil {
		t.Fatal(err)
	}
	env := []string{}
	for _, value := range os.Environ() {
		if !strings.HasPrefix(value, "UNIC_") {
			env = append(env, value)
		}
	}
	var proc *exec.Cmd
	var logs bytes.Buffer
	stop := func() {
		if proc == nil {
			return
		}
		if e := proc.Process.Signal(os.Interrupt); e != nil {
			t.Error("cannot signal owned service")
		}
		done := make(chan error, 1)
		go func() { done <- proc.Wait() }()
		var waitErr error
		forcedKill := false
		select {
		case waitErr = <-done:
		case <-time.After(10 * time.Second):
			t.Error("owned service shutdown exceeded budget")
			if killErr := proc.Process.Kill(); killErr != nil {
				if !errors.Is(killErr, os.ErrProcessDone) {
					t.Errorf("cannot force-stop owned service: %v", killErr)
				}
			} else {
				forcedKill = true
			}
			waitErr = <-done
		}
		if waitErr != nil {
			var exitErr *exec.ExitError
			if !forcedKill || !errors.As(waitErr, &exitErr) {
				t.Errorf("owned service shutdown failed: %v", waitErr)
			}
		}
		proc = nil
	}
	defer stop()
	client := api.NewClient(filepath.Join(home, "unic.sock"))
	start := func() {
		t.Helper()
		logs.Reset()
		proc = exec.CommandContext(ctx, exe)
		proc.Env = append(env, "UNIC_HOME="+home, "UNIC_RENDEZVOUS_URL=http://127.0.0.1:1")
		proc.Stdout, proc.Stderr = &logs, &logs
		if e := proc.Start(); e != nil {
			t.Fatal(e)
		}
		deadline := time.Now().Add(20 * time.Second)
		for time.Now().Before(deadline) {
			if _, e := client.Status(ctx); e == nil {
				return
			}
			time.Sleep(20 * time.Millisecond)
		}
		t.Fatal("owned service did not become ready")
	}
	start()
	desktop := NewDesktop(ctx, client)
	defer desktop.Close()
	d := desktop.d
	drain := func(ready func() bool) {
		t.Helper()
		deadline := time.NewTimer(10 * time.Second)
		defer deadline.Stop()
		for !ready() {
			select {
			case apply := <-d.live.results:
				apply()
			case <-deadline.C:
				t.Fatal("storage completion timed out")
			}
		}
	}
	d.refreshStorageUsage()
	d.refreshStorageUsage()
	drain(func() bool { return !d.storage.loading })
	if !d.storage.loaded || d.storage.message != "" || d.storage.storedBytes != 0 || d.storage.reservedBytes != 0 || d.storage.fileCount != 0 || d.storage.customDestination {
		t.Fatal("initial cache inventory differs from the empty ledger")
	}
	checks = append(checks, "real-service default-cache ledger read and duplicate refresh coalescing")
	if got, e := os.ReadFile(unknown); e != nil || string(got) != "not a ledger payload" {
		t.Fatal("unknown user file changed")
	}
	if got, e := os.ReadFile(configPath); e != nil || string(got) != configText {
		t.Fatal("inventory changed configuration")
	}
	checks = append(checks, "unknown files ignored and preserved; read leaves config bytes intact")
	opened := 0
	desktop.SetStorageDirectoryOpener(func() error { opened++; return nil })
	d.openStorageDirectory()
	drain(func() bool { return !d.storage.opening })
	if opened != 1 || d.storage.openMessage != "" {
		t.Fatal("directory action did not invoke the host callback")
	}
	desktop.SetStorageDirectoryOpener(func() error { return errors.New("private /secret/profile should not appear") })
	d.openStorageDirectory()
	drain(func() bool { return !d.storage.opening })
	if d.storage.openMessage == "" || strings.Contains(d.storage.openMessage, "/secret") {
		t.Fatal("directory failure missing or leaked private error")
	}
	checks = append(checks, "directory host callback and sanitized failure without file-manager launch")
	stop()
	configText = "inbox=" + "\"" + filepath.ToSlash(custom) + "\"\n" + configText
	if err = os.WriteFile(configPath, []byte(configText), 0600); err != nil {
		t.Fatal(err)
	}
	start()
	d.refreshStorageUsage()
	drain(func() bool { return !d.storage.loading })
	if !d.storage.loaded || !d.storage.customDestination || d.storage.storedBytes != 0 || d.storage.fileCount != 0 {
		t.Fatal("custom destination was counted as managed cache")
	}
	if got, e := os.ReadFile(customFile); e != nil || string(got) != "preserve custom bytes" {
		t.Fatal("custom user file changed")
	}
	checks = append(checks, "custom destination correctly excluded after real service restart")
	stop()
	d.refreshStorageUsage()
	drain(func() bool { return !d.storage.loading })
	if d.storage.message == "" || !d.storage.loaded {
		t.Fatal("failed refresh lost prior sample or reported fresh success")
	}
	checks = append(checks, "unavailable service exposes refresh failure with previous sample retained")
}

package desktopprocess_test

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"testing"
	"time"
)

func TestRoleProcesses(t *testing.T) {
	if os.Getenv("UNIC_ROLE_E2E") != "1" {
		t.Skip("real OS processes: UNIC_ROLE_E2E=1")
	}
	dir := t.TempDir()
	binary := filepath.Join(dir, "role")
	if runtime.GOOS == "windows" {
		binary += ".exe"
	}
	cmd := exec.Command("go", "build", "-race", "-o", binary, "./testdata/role.go")
	if raw, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("build: %v\n%s", err, raw)
	}
	profile := filepath.Join(dir, "profile")
	if err := os.Mkdir(profile, 0700); err != nil {
		t.Fatal(err)
	}
	launch := func(role string) *exec.Cmd {
		t.Helper()
		child := exec.Command(binary, "--role", role)
		child.Env = append(os.Environ(), "UNIC_HOME="+profile)
		if err := child.Start(); err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = child.Process.Kill(); _ = child.Wait() })
		return child
	}
	call := func(role, command string) (map[string]any, error) {
		ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		defer cancel()
		child := exec.CommandContext(ctx, binary, "--role", role, "--command", command)
		child.Env = append(os.Environ(), "UNIC_HOME="+profile)
		raw, err := child.CombinedOutput()
		var result map[string]any
		if err == nil {
			err = json.Unmarshal(raw, &result)
		}
		return result, err
	}
	wait := func(role string) map[string]any {
		t.Helper()
		deadline := time.Now().Add(10 * time.Second)
		for time.Now().Before(deadline) {
			if result, err := call(role, "status"); err == nil {
				return result
			}
			time.Sleep(25 * time.Millisecond)
		}
		t.Fatalf("role %s unavailable", role)
		return nil
	}
	tray := launch("tray")
	gui := launch("gui")
	panel := launch("quick-panel")
	receipt := map[string]any{"native_ui": false, "processes": []any{wait("tray"), wait("gui"), wait("quick-panel")}}
	sources := map[string]string{}
	for _, path := range []string{"process.go", "process_e2e_test.go", "testdata/role.go", "FAILURES.md"} {
		raw, err := os.ReadFile(path)
		if err != nil {
			t.Fatal(err)
		}
		sources[path] = fmt.Sprintf("%x", sha256.Sum256(raw))
	}
	receipt["source_sha256"] = sources
	defer func() {
		receipt["status"] = "PASS"
		if t.Failed() {
			receipt["status"] = "FAIL"
		}
		raw, _ := json.MarshalIndent(receipt, "", "  ")
		if output := os.Getenv("UNIC_ROLE_E2E_RECEIPT"); output != "" {
			if err := os.WriteFile(output, raw, 0600); err != nil {
				t.Error(err)
			}
		}
		t.Log(string(raw))
	}()
	retiring, err := call("gui", "classify-retiring")
	if err != nil {
		t.Fatal(err)
	}
	receipt["retiring_error_classification"] = retiring
	if retiring["received_error"] != true || retiring["retryable_unavailable"] != true {
		t.Fatalf("remote retiring error lost unavailable classification: %v", retiring)
	}
	lookalike, err := call("gui", "classify-lookalike")
	if err != nil {
		t.Fatal(err)
	}
	receipt["lookalike_error_classification"] = lookalike
	if lookalike["received_error"] != true || lookalike["retryable_unavailable"] != false {
		t.Fatalf("human-readable error text became retryable: %v", lookalike)
	}
	if tray.Process.Pid == gui.Process.Pid || tray.Process.Pid == panel.Process.Pid || gui.Process.Pid == panel.Process.Pid {
		t.Fatal("roles share PID")
	}
	rogue := filepath.Join(dir, "rogue")
	if runtime.GOOS == "windows" {
		rogue += ".exe"
	}
	if raw, err := exec.Command("go", "build", "-o", rogue, "./testdata/rogue").CombinedOutput(); err != nil {
		t.Fatalf("build rogue: %v %s", err, raw)
	}
	identity, _ := json.Marshal(wait("tray"))
	attack := exec.Command(rogue)
	attack.Stdin = bytes.NewReader(identity)
	if raw, err := attack.CombinedOutput(); err != nil {
		t.Fatalf("peer binary forgery was accepted: %v %s", err, raw)
	}
	receipt["forged_binary_identity"] = "rejected"
	duplicate := launch("tray")
	done := make(chan error, 1)
	go func() { done <- duplicate.Wait() }()
	select {
	case err := <-done:
		if err == nil {
			t.Fatal("duplicate owner accepted")
		}
	case <-time.After(5 * time.Second):
		t.Fatal("duplicate blocked")
	}
	_ = gui.Process.Kill()
	_ = gui.Wait()
	gui = launch("gui")
	if result := wait("gui"); int(result["pid"].(float64)) != gui.Process.Pid {
		t.Fatal("stale GUI owner")
	}
	if result := wait("tray"); int(result["pid"].(float64)) != tray.Process.Pid {
		t.Fatal("GUI crash changed tray")
	}
	for _, role := range []string{"gui", "quick-panel", "tray"} {
		if _, err := call(role, "close"); err != nil {
			t.Fatal(err)
		}
	}
	time.Sleep(250 * time.Millisecond)
	if _, err := call("tray", "status"); err == nil {
		t.Fatal("closed tray still accepts commands")
	}
	receipt["cleanup"] = "role endpoints closed"
}

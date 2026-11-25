package desktopprocess

import (
	"context"
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"testing"
	"time"
)

func TestIsReplacedExecutable(t *testing.T) {
	dir := t.TempDir()
	exe := filepath.Join(dir, "unic-desktop.exe")
	otherDir := t.TempDir()

	cases := []struct {
		name     string
		peer     string
		expected string
		want     bool
	}{
		{"moved aside numeric pid", filepath.Join(dir, ".unic-desktop.exe.old-12345"), exe, true},
		{"linux deleted", exe + " (deleted)", exe, true},
		{"macos staged bundle", filepath.Join(dir, ".UniClipboard.app.update-123", "old", "Contents", "MacOS", "unic-desktop"), filepath.Join(dir, "UniClipboard.app", "Contents", "MacOS", "unic-desktop"), true},
		{"rogue sibling binary", filepath.Join(dir, "rogue.exe"), exe, false},
		{"rogue moved aside", filepath.Join(dir, ".rogue.exe.old-12345"), exe, false},
		{"different directory moved aside", filepath.Join(otherDir, ".unic-desktop.exe.old-12345"), exe, false},
		{"non-numeric old suffix", filepath.Join(dir, ".unic-desktop.exe.old-abc"), exe, false},
		{"missing old suffix", filepath.Join(dir, ".unic-desktop.exe.old-"), exe, false},
		{"unrelated backup suffix", filepath.Join(dir, "unic-desktop.exe.bak"), exe, false},
		{"macos staged wrong binary", filepath.Join(dir, ".UniClipboard.app.update-123", "old", "Contents", "MacOS", "rogue"), filepath.Join(dir, "UniClipboard.app", "Contents", "MacOS", "unic-desktop"), false},
		{"empty paths", "", exe, false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if got := isReplacedExecutable(tc.peer, tc.expected); got != tc.want {
				t.Fatalf("isReplacedExecutable(%q, %q) = %v, want %v", tc.peer, tc.expected, got, tc.want)
			}
		})
	}
}

func TestRetireStaleViewRole(t *testing.T) {
	dir := t.TempDir()
	bin := filepath.Join(dir, "role")
	if runtime.GOOS == "windows" {
		bin += ".exe"
	}
	build := func(dst, tag string) {
		t.Helper()
		cmd := exec.Command("go", "build", "-ldflags", "-X main.buildTag="+tag, "-o", dst, "./testdata/role.go")
		if raw, err := cmd.CombinedOutput(); err != nil {
			t.Fatalf("build %s (%s): %v\n%s", dst, tag, err, raw)
		}
	}
	build(bin, "v1")

	profile := filepath.Join(dir, "profile")
	if err := os.Mkdir(profile, 0700); err != nil {
		t.Fatal(err)
	}
	runCmd := func(exe, role, command string) map[string]any {
		t.Helper()
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		cmd := exec.CommandContext(ctx, exe, "--role", role, "--command", command)
		cmd.Env = append(os.Environ(), "UNIC_HOME="+profile)
		raw, err := cmd.CombinedOutput()
		if err != nil {
			t.Fatalf("%s --role %s --command %s: %v\n%s", exe, role, command, err, raw)
		}
		var out map[string]any
		if err := json.Unmarshal(raw, &out); err != nil {
			t.Fatalf("decode %q: %v", raw, err)
		}
		return out
	}
	startRole := func(exe, role string) *exec.Cmd {
		t.Helper()
		child := exec.Command(exe, "--role", role)
		child.Env = append(os.Environ(), "UNIC_HOME="+profile)
		if err := child.Start(); err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = child.Process.Kill(); _ = child.Wait() })
		deadline := time.Now().Add(10 * time.Second)
		for time.Now().Before(deadline) {
			ctx, cancel := context.WithTimeout(context.Background(), time.Second)
			cmd := exec.CommandContext(ctx, exe, "--role", role, "--command", "status")
			cmd.Env = append(os.Environ(), "UNIC_HOME="+profile)
			err := cmd.Run()
			cancel()
			if err == nil {
				return child
			}
			time.Sleep(25 * time.Millisecond)
		}
		t.Fatalf("role %s did not become ready", role)
		return nil
	}

	// Start v1 quick-panel, then move the executable aside exactly as mygo.Updater swapFiles does.
	oldPanel := startRole(bin, "quick-panel")
	aside := filepath.Join(dir, "."+filepath.Base(bin)+".old-99999")
	if err := os.Rename(bin, aside); err != nil {
		t.Fatal(err)
	}
	build(bin, "v2")

	// v2 must classify the orphaned v1 listener as ErrStaleBuild.
	probe := runCmd(bin, "quick-panel", "probe-stale")
	if probe["stale"] != true {
		t.Fatalf("expected stale build classification after swap, got %v", probe)
	}

	// Tray retirement is refused.
	trayRetire := runCmd(bin, "tray", "retire-stale")
	if trayRetire["retired"] != false || trayRetire["error"] == "" {
		t.Fatalf("expected tray retirement refusal, got %v", trayRetire)
	}

	// Retiring the stale quick-panel must terminate v1 and release the endpoint.
	retired := runCmd(bin, "quick-panel", "retire-stale")
	if retired["retired"] != true || retired["error"] != "" {
		t.Fatalf("expected stale quick-panel retirement, got %v", retired)
	}
	_ = oldPanel.Wait()

	after := runCmd(bin, "quick-panel", "probe-stale")
	if after["unavailable"] != true {
		t.Fatalf("expected quick-panel unavailable after retirement, got %v", after)
	}

	// v2 can now acquire quick-panel, and RetireStale must leave the live v2 process untouched.
	newPanel := startRole(bin, "quick-panel")
	keep := runCmd(bin, "quick-panel", "retire-stale")
	if keep["retired"] != false || keep["error"] != "" {
		t.Fatalf("expected current-build quick-panel to remain alive, got %v", keep)
	}
	status := runCmd(bin, "quick-panel", "status")
	if int(status["pid"].(float64)) != newPanel.Process.Pid {
		t.Fatalf("current-build quick-panel PID changed: got %v want %d", status["pid"], newPanel.Process.Pid)
	}
	_ = runCmd(bin, "quick-panel", "close")
	_ = newPanel.Wait()

	// An unrecognized binary in the same directory must NOT be classified as stale or retired.
	rogue := filepath.Join(dir, "other")
	if runtime.GOOS == "windows" {
		rogue += ".exe"
	}
	build(rogue, "rogue")
	roguePanel := startRole(rogue, "quick-panel")
	rogueProbe := runCmd(bin, "quick-panel", "probe-stale")
	if rogueProbe["stale"] != false {
		t.Fatalf("unrecognized executable must not be classified as stale: %v", rogueProbe)
	}
	rogueRetire := runCmd(bin, "quick-panel", "retire-stale")
	if rogueRetire["retired"] != false || rogueRetire["error"] == "" {
		t.Fatalf("unrecognized executable must not be retired: %v", rogueRetire)
	}
	_ = runCmd(rogue, "quick-panel", "close")
	_ = roguePanel.Wait()
}

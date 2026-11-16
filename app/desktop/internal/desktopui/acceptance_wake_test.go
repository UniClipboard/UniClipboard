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

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/internal/clip"
	api "github.com/uniclipboard/unic/internal/localapi"
)

// A real service, production bindings and the renderer exercise scheduling.
// Advancing the renderer's clock catches future After requests that the native
// automation idle endpoint intentionally does not consider pending yet.
func scenarioEventWakeup(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Minute)
	defer cancel()
	private, err := os.MkdirTemp("/tmp", "unic-wake-")
	if err != nil {
		t.Fatal(err)
	}
	defer func() {
		if err := os.RemoveAll(private); err != nil {
			t.Error(err)
		}
	}()
	exe := filepath.Join(private, "unicd")
	root, err := filepath.Abs("../../../..")
	if err != nil {
		t.Fatal(err)
	}
	build := exec.CommandContext(ctx, "go", "build", "-race", "-o", exe, "./app/service/cmd/unicd")
	build.Dir = root
	if out, err := build.CombinedOutput(); err != nil {
		t.Fatalf("build: %v\n%s", err, out)
	}
	binary, err := os.ReadFile(exe)
	if err != nil {
		t.Fatal(err)
	}
	sum := sha256.Sum256(binary)
	receipt := map[string]any{"passed": false, "unicd_sha256": hex.EncodeToString(sum[:]), "native_ui": false, "renderer": true}
	checks := []string{}
	defer func() {
		receipt["passed"] = !t.Failed()
		receipt["checks"] = checks
		data, err := json.MarshalIndent(receipt, "", "  ")
		if err != nil {
			t.Error(err)
			return
		}
		if err := os.WriteFile(os.Getenv("UNIC_DESKTOP_E2E_CASE_RECEIPT"), data, 0600); err != nil {
			t.Error(err)
		}
	}()
	home := filepath.Join(private, "home")
	if err := os.MkdirAll(home, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(home, "config.toml"), []byte("[telemetry]\nenabled=false\n[sync]\nenabled=false\n"), 0600); err != nil {
		t.Fatal(err)
	}
	proc := exec.CommandContext(ctx, exe)
	for _, value := range os.Environ() {
		if !strings.HasPrefix(value, "UNIC_") {
			proc.Env = append(proc.Env, value)
		}
	}
	proc.Env = append(proc.Env, "UNIC_HOME="+home, "UNIC_RENDEZVOUS_URL=http://127.0.0.1:1")
	if err := proc.Start(); err != nil {
		t.Fatal(err)
	}
	defer func() { _ = proc.Process.Kill(); _ = proc.Wait() }()
	client := api.NewClient(filepath.Join(home, "unic.sock"))
	wait := func(label string, ready func() bool) {
		t.Helper()
		deadline := time.Now().Add(15 * time.Second)
		for time.Now().Before(deadline) {
			if ready() {
				return
			}
			time.Sleep(10 * time.Millisecond)
		}
		t.Fatalf("timeout: %s", label)
	}
	wait("service", func() bool { _, err := client.Status(ctx); return err == nil })
	submit := func(text string) {
		t.Helper()
		_, err := client.Submit(ctx, api.SubmitInput{RequestID: text, Snapshot: clip.Snapshot{Parts: []clip.Part{{Format: "text/plain", Data: []byte(text)}}}})
		if err != nil {
			t.Fatal(err)
		}
	}
	submit("wake initial text")
	desktop := NewDesktop(ctx, client)
	defer desktop.Close()
	frames := 0
	view := ui.NewTester(func(c *ui.Context) { frames++; desktop.View(c) }, 1100, 720)
	wait("initial async completion", func() bool {
		view.Advance(20 * time.Millisecond)
		return desktop.d.live.loaded && !desktop.d.live.loading && len(desktop.d.live.detailReads) == 0 && desktop.d.current() != nil && desktop.d.current().text == "wake initial text"
	})
	// Let all initial query/count/catalogue completions and component motion end.
	for n := 0; n < 30; n++ {
		time.Sleep(10 * time.Millisecond)
		view.Advance(100 * time.Millisecond)
	}
	before := frames
	view.Advance(2 * time.Second)
	if frames != before {
		t.Fatalf("idle renderer rebuilt %d times without changes", frames-before)
	}
	checks = append(checks, "two seconds of future UI time produce no idle heartbeat frames")
	submit("wake asynchronous event")
	wait("service event and result wake", func() bool {
		view.Advance(20 * time.Millisecond)
		for _, v := range desktop.d.entries {
			if v.text == "wake asynchronous event" {
				return true
			}
		}
		return false
	})
	checks = append(checks, "new service history and detail completion appear without manual frames or polling")

	quick := newLiveQuick(ctx, client, "system", nil)
	defer quick.live.d.live.cancel()
	quickFrames := 0
	panel := ui.NewTester(func(c *ui.Context) { quickFrames++; quick.panel(c) }, 380, 366)
	wait("quick-panel initial results", func() bool {
		panel.Advance(20 * time.Millisecond)
		return quick.live.d.live.loaded && !quick.live.d.live.loading && len(quick.live.d.live.detailReads) == 0
	})
	for n := 0; n < 30; n++ {
		time.Sleep(10 * time.Millisecond)
		panel.Advance(100 * time.Millisecond)
	}
	panel.SetFocused(false)
	panel.Advance(time.Second)
	before = quickFrames
	panel.Advance(2 * time.Second)
	if quickFrames != before {
		t.Fatalf("idle quick panel rebuilt %d times", quickFrames-before)
	}
	submit("wake quick-panel event")
	wait("quick-panel event delivery", func() bool {
		panel.Advance(20 * time.Millisecond)
		for _, v := range quick.rows {
			if v.text == "wake quick-panel event" {
				return true
			}
		}
		return false
	})
	checks = append(checks, "quick panel settles and wakes for service events independently of its preview satellite")

	// Deadline-based recovery must wake a stopped renderer without After.
	serial := desktop.d.live.appliedSerial
	desktop.d.live.nextRefresh = time.Now().Add(50 * time.Millisecond)
	view.Frame()
	wait("recovery deadline", func() bool {
		view.Advance(20 * time.Millisecond)
		return !desktop.d.live.loading && desktop.d.live.nextRefresh.After(time.Now().Add(time.Second))
	})
	checks = append(checks, "independent recovery deadline runs while no heartbeat frames are scheduled")
	receipt["frames"] = frames
	receipt["quick_panel_frames"] = quickFrames
	receipt["recovery_serial_before"] = serial
	receipt["recovery_serial_after"] = desktop.d.live.appliedSerial
	checks = append(checks, fmt.Sprintf("real service and both production bindings completed; frames=%d quick_frames=%d", frames, quickFrames))
}

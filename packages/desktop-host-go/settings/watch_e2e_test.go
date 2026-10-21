//go:build e2e

// End-to-end check of the settings watcher against a real `uniclipd` on an isolated profile
// (ADR-013 section 7, F26 and F29–F37). No GUI and no WebView run: the watcher is the code the GUI
// host uses, and each scenario repeats the order of calls the host makes (Pause before a daemon
// stop, Resume after a failed restart or relaunch).
//
// Run from packages/desktop-host-go after `cargo build -p uc-daemon`:
//
//	UC_E2E_UNICLIPD=$PWD/../../target/debug/uniclipd UC_E2E_ARTIFACT_DIR=<dir> \
//	  go test -tags e2e -run TestWatcherAgainstDaemon -count=1 -v ./settings
package settings

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/apppaths"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonproc"
)

type daemonProcess struct {
	cmd  *exec.Cmd
	done chan struct{}
}

type harness struct {
	t       *testing.T
	binary  string
	logPath string
	daemon  *daemonProcess
	applied chan View
	steps   []map[string]any
}

func (h *harness) note(step string, detail map[string]any) {
	h.t.Logf("%s %v", step, detail)
	h.steps = append(h.steps, map[string]any{"step": step, "atMs": time.Now().UnixMilli(), "detail": detail})
}

func (h *harness) start() {
	h.t.Helper()
	log, err := os.OpenFile(h.logPath, os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0o600)
	if err != nil {
		h.t.Fatal(err)
	}
	cmd := exec.Command(h.binary)
	level := os.Getenv("UC_E2E_RUST_LOG")
	if level == "" {
		level = "warn,uc_webserver::api::ws=debug" // the closed-connection line proves Pause released the lease
	}
	cmd.Env = append(os.Environ(), "UC_DAEMON_RUN_MODE=server", "RUST_LOG="+level)
	cmd.Stdout, cmd.Stderr = log, log
	if err := cmd.Start(); err != nil {
		h.t.Fatal(err)
	}
	process := &daemonProcess{cmd: cmd, done: make(chan struct{})}
	go func() { _ = cmd.Wait(); log.Close(); close(process.done) }()
	h.daemon = process
	deadline := time.Now().Add(60 * time.Second)
	for time.Now().Before(deadline) {
		conn, _ := daemonproc.ReadConnFile()
		if conn != nil && int(conn.PID) == cmd.Process.Pid {
			if client, err := daemonclient.FromEnv(); err == nil {
				ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
				_, err = Read(ctx, client)
				cancel()
				if err == nil {
					h.note("daemon started", map[string]any{"pid": cmd.Process.Pid, "port": conn.Port})
					return
				}
			}
		}
		time.Sleep(200 * time.Millisecond)
	}
	h.t.Fatal("daemon did not become ready")
}

func (h *harness) kill() {
	_ = h.daemon.cmd.Process.Kill()
	<-h.daemon.done
	h.note("daemon killed", nil)
}

// gracefulStop asks the daemon to stop and returns how long it took to exit.
func (h *harness) gracefulStop() time.Duration {
	h.t.Helper()
	client := h.client()
	began := time.Now()
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	resp, err := client.Send(ctx, daemonclient.Request{Method: http.MethodPost, Path: "/lifecycle/graceful-stop"})
	if err != nil {
		h.t.Fatal(err)
	}
	resp.Body.Close()
	select {
	case <-h.daemon.done:
	case <-time.After(30 * time.Second):
		h.t.Fatal("daemon did not exit after graceful stop")
	}
	return time.Since(began)
}

// closedConnections counts WebSocket connections the daemon has seen close, from its log.
func (h *harness) closedConnections() int {
	data, _ := os.ReadFile(h.logPath)
	return strings.Count(string(data), "daemon websocket connection closed")
}

// pause pauses the watcher and waits until the daemon has closed the watcher's connection.
func (h *harness) pause(watcher *Watcher, step string) {
	h.t.Helper()
	before := h.closedConnections()
	began := time.Now()
	watcher.Pause()
	for time.Since(began) < 3*time.Second {
		if h.closedConnections() > before {
			h.note(step, map[string]any{"connectionClosedAfterMs": time.Since(began).Milliseconds()})
			return
		}
		time.Sleep(20 * time.Millisecond)
	}
	h.t.Fatalf("%s: the daemon did not see the watcher connection close", step)
}

func (h *harness) client() *daemonclient.Client {
	h.t.Helper()
	client, err := daemonclient.FromEnv()
	if err != nil {
		h.t.Fatal(err)
	}
	return client
}

func (h *harness) put(body map[string]any) {
	h.t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	if err := h.client().Enveloped(ctx, daemonclient.Request{Method: http.MethodPut, Path: "/settings", JSON: body}, nil); err != nil {
		h.t.Fatalf("PUT /settings %v: %v", body, err)
	}
	h.note("external write", map[string]any{"body": body})
}

// waitApplied waits for an apply call whose view satisfies want.
func (h *harness) waitApplied(step string, timeout time.Duration, want func(View) bool) View {
	h.t.Helper()
	began := time.Now()
	deadline := time.After(timeout)
	for {
		select {
		case view := <-h.applied:
			if want(view) {
				h.note(step, map[string]any{"afterMs": time.Since(began).Milliseconds(), "syncEnabled": view.Sync.SyncEnabled})
				return view
			}
		case <-deadline:
			h.t.Fatalf("%s: no matching apply within %s", step, timeout)
		}
	}
}

// expectNoApply checks that the watcher applies nothing while it is paused.
func (h *harness) expectNoApply(step string, window time.Duration) {
	h.t.Helper()
	select {
	case view := <-h.applied:
		h.t.Fatalf("%s: unexpected apply %+v", step, view)
	case <-time.After(window):
		h.note(step, map[string]any{"windowMs": window.Milliseconds()})
	}
}

func (h *harness) drain() {
	for {
		select {
		case <-h.applied:
		default:
			return
		}
	}
}

func TestWatcherAgainstDaemon(t *testing.T) {
	binary := os.Getenv("UC_E2E_UNICLIPD")
	if binary == "" {
		t.Skip("UC_E2E_UNICLIPD is not set")
	}
	suffix := make([]byte, 6)
	_, _ = rand.Read(suffix)
	t.Setenv("UC_PROFILE", "e2e-settings-watch-"+hex.EncodeToString(suffix))
	t.Setenv("UNICLIPBOARD_ENV", "development")
	dataRoot, _ := apppaths.AppDataRoot()
	cacheRoot, _ := apppaths.AppCacheRoot()
	logDir, _ := apppaths.AppLogDir()
	artifactDir := os.Getenv("UC_E2E_ARTIFACT_DIR")
	if artifactDir == "" {
		artifactDir = t.TempDir()
	}
	if err := os.MkdirAll(artifactDir, 0o755); err != nil {
		t.Fatal(err)
	}
	h := &harness{t: t, binary: binary, logPath: filepath.Join(artifactDir, "settings-watch-daemon.log"), applied: make(chan View, 256)}
	_ = os.Remove(h.logPath)
	defer func() {
		if h.daemon != nil {
			select {
			case <-h.daemon.done:
			default:
				h.kill()
			}
		}
		for _, dir := range []string{dataRoot, cacheRoot, logDir} {
			if dir != "" {
				_ = os.RemoveAll(dir)
			}
		}
		record := map[string]any{"test": t.Name(), "profile": os.Getenv("UC_PROFILE"), "passed": !t.Failed(), "steps": h.steps}
		data, _ := json.MarshalIndent(record, "", "  ")
		_ = os.WriteFile(filepath.Join(artifactDir, "settings-watch.json"), data, 0o644)
	}()

	// Baseline: how long a graceful stop takes with no WebSocket connected at all.
	h.start()
	baseline := h.gracefulStop()
	h.note("graceful stop without any connection (baseline)", map[string]any{"ms": baseline.Milliseconds()})
	h.start()

	// F36: every field of the shared view decodes from a real response, checked with non-default values.
	channel := "beta"
	h.put(map[string]any{
		"general": map[string]any{"autoStart": true, "startupMode": "silent", "restoreLastEntryOnStartup": true,
			"deviceName": "e2e-watch-device", "autoCheckUpdate": false, "autoDownloadUpdate": true, "updateChannel": channel, "trayIcon": "cat"},
		"sync":       map[string]any{"syncEnabled": false},
		"quickPanel": map[string]any{"enabled": false, "position": "follow_cursor", "doubleTapModifier": "alt"},
	})
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	view, err := Read(ctx, h.client())
	cancel()
	if err != nil {
		t.Fatal(err)
	}
	want := General{AutoStart: true, StartupMode: "silent", RestoreLastEntryOnStartup: true, DeviceName: "e2e-watch-device",
		AutoCheckUpdate: false, AutoDownloadUpdate: true, TrayIcon: "cat"}
	got := view.General
	got.UpdateChannel = nil
	if got != want || view.General.UpdateChannel == nil || *view.General.UpdateChannel != channel ||
		view.Sync.SyncEnabled || view.QuickPanel != (QuickPanel{Enabled: false, Position: "follow_cursor", DoubleTapModifier: "alt"}) ||
		view.KeyboardShortcuts == nil {
		t.Fatalf("shared view decoded %+v (channel %v)", view, view.General.UpdateChannel)
	}
	h.note("shared view decoded", map[string]any{"view": view})

	// F33: the watcher alone (no frontend) reads at connect and follows external writes of both owners.
	// The host's provider: the stored client, followed to whichever daemon daemon.conn names. The test never stores
	// a new client itself, so every recovery below depends on Follow, as in a host whose restart reported a failure.
	var holder atomic.Pointer[daemonclient.Client]
	holder.Store(h.client())
	watcher := NewWatcher(func() *daemonclient.Client { return daemonclient.Follow(&holder) }, func(v View) { h.applied <- v })
	runCtx, stop := context.WithCancel(context.Background())
	defer stop()
	go watcher.Run(runCtx)
	h.waitApplied("initial read after snapshot", 10*time.Second, func(v View) bool { return !v.Sync.SyncEnabled })
	h.put(map[string]any{"sync": map[string]any{"syncEnabled": true}})
	h.waitApplied("engine-owned external write applied", 5*time.Second, func(v View) bool { return v.Sync.SyncEnabled })
	h.put(map[string]any{"general": map[string]any{"trayIcon": "glyph"}}) // F36 left "cat", so this is a change
	h.waitApplied("product-owned external write applied", 5*time.Second, func(v View) bool { return v.General.TrayIcon == "glyph" })

	// F32, measured: an open watcher connection at a daemon stop, which the host avoids with Pause.
	held := h.gracefulStop()
	h.note("graceful stop with the watcher connected", map[string]any{"ms": held.Milliseconds()})

	// F29/F34/F37: unpaused reconnect to a new process (new port and token).
	h.start()
	h.put(map[string]any{"sync": map[string]any{"syncEnabled": false}})
	h.waitApplied("reconnected after daemon replacement", 20*time.Second, func(v View) bool { return !v.Sync.SyncEnabled })

	// Exit path: Pause closes the connection (the daemon logs it) before the stop. The stop time is recorded next to
	// the baseline; it is not a proof by itself because the stop takes about as long without any connection.
	h.pause(watcher, "Pause closed the connection (exit path)")
	paused := h.gracefulStop()
	h.note("graceful stop after Pause", map[string]any{"ms": paused.Milliseconds(), "baselineMs": baseline.Milliseconds()})

	// Failed daemon restart (RestartDaemon resumes in a defer and stores no new client): no daemon comes up, the
	// watcher resumes and retries; when a daemon is back it reads the current state. A second watcher that keeps the
	// stale client (the provider without Follow) is the negative control: it must not recover.
	stale := holder.Load()
	staleApplied := make(chan View, 64)
	staleWatcher := NewWatcher(func() *daemonclient.Client { return stale }, func(v View) { staleApplied <- v })
	staleCtx, stopStale := context.WithCancel(context.Background())
	go staleWatcher.Run(staleCtx)
	h.drain()
	watcher.Resume()
	h.note("resumed with no daemon (failed restart)", nil)
	time.Sleep(3 * time.Second) // several failed attempts with backoff
	h.start()
	h.put(map[string]any{"sync": map[string]any{"syncEnabled": true}})
	h.waitApplied("recovered after a failed restart (Follow)", 40*time.Second, func(v View) bool { return v.Sync.SyncEnabled })
	select {
	case v := <-staleApplied:
		t.Fatalf("negative control: a watcher with the stale client applied %+v", v)
	case <-time.After(10 * time.Second):
		h.note("negative control: the stale client never recovers", map[string]any{"windowMs": 10000})
	}
	stopStale()

	// Full restart or update whose relaunch failed: Pause, stop, a new daemon, a write while paused, Resume.
	// The value written while paused comes from the read after the snapshot, not from an event.
	h.pause(watcher, "Pause closed the connection (restart or update path)")
	h.drain()
	h.gracefulStop()
	h.start()
	h.put(map[string]any{"sync": map[string]any{"syncEnabled": false}})
	h.expectNoApply("paused watcher applies nothing", 2*time.Second)
	watcher.Resume()
	h.waitApplied("resumed after restart sees the write made while paused", 10*time.Second, func(v View) bool { return !v.Sync.SyncEnabled })

	// Still live after all of it.
	h.put(map[string]any{"sync": map[string]any{"syncEnabled": true}})
	h.waitApplied("live after resume", 5*time.Second, func(v View) bool { return v.Sync.SyncEnabled })

	h.pause(watcher, "Pause closed the connection (final exit)")
	final := h.gracefulStop()
	h.note("final graceful stop after Pause", map[string]any{"ms": final.Milliseconds()})
	h.note("completed", nil)
}

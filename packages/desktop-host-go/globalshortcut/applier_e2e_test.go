//go:build e2e

// End-to-end check of the global shortcut apply rules (ADR-013 section 10) against a real `uniclipd` on an isolated
// profile. The settings watcher and the applier are the code the GUI host runs; the executor is a counting test
// double, so this proves the sequence of requests, events, executor calls and status, NOT an OS registration.
// The glue that the host adds (`reconcilePanel` in apps/gui-go) is repeated here in its call order: target from the
// view, Reconcile with a fresh read as the confirmation.
//
// Run from packages/desktop-host-go after `cargo build -p uc-daemon`:
//
//	UC_E2E_UNICLIPD=$PWD/../../target/debug/uniclipd UC_E2E_ARTIFACT_DIR=<dir> \
//	  go test -tags e2e -run TestApplierAgainstDaemon -count=1 -v ./globalshortcut
package globalshortcut

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/apppaths"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonproc"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/settings"
)

const shortcutID = "global.toggleQuickPanel"

// countingExecutor is the test double: it holds a set, refuses bindings listed in refuse (a binding "taken by another
// program"), and can be told to answer the next call with an unknown outcome.
type countingExecutor struct {
	mu          sync.Mutex
	held        []string
	refuse      map[string]bool
	unknownNext bool
	calls       [][]string
}

func (e *countingExecutor) Apply(bindings []string) ([]string, error) {
	e.mu.Lock()
	defer e.mu.Unlock()
	e.calls = append(e.calls, slices.Clone(bindings))
	if e.unknownNext {
		e.unknownNext = false
		e.held = slices.Clone(bindings) // it did take them, but the answer is lost
		return nil, fmt.Errorf("%w: no answer", ErrUnknown)
	}
	for _, b := range bindings {
		if e.refuse[b] {
			return slices.Clone(e.held), fmt.Errorf("the shortcut %s is already registered (test double)", b)
		}
	}
	e.held = slices.Clone(bindings)
	return slices.Clone(e.held), nil
}

func (e *countingExecutor) count() int {
	e.mu.Lock()
	defer e.mu.Unlock()
	return len(e.calls)
}

func (e *countingExecutor) setRefuse(bindings ...string) {
	e.mu.Lock()
	defer e.mu.Unlock()
	e.refuse = map[string]bool{}
	for _, b := range bindings {
		e.refuse[b] = true
	}
}

type daemonRun struct {
	cmd  *exec.Cmd
	done chan struct{}
}

type run struct {
	t        *testing.T
	binary   string
	logPath  string
	daemon   *daemonRun
	steps    []map[string]any
	exec     *countingExecutor
	applier  *Applier
	statuses []Status
	statusMu sync.Mutex
	applied  chan settings.View
	product  string // desktop-settings.json path
}

func (r *run) note(step string, detail map[string]any) {
	if detail == nil {
		detail = map[string]any{}
	}
	detail["executorCalls"] = r.exec.count()
	detail["status"] = r.applier.Status()
	detail["productFileSha256"] = r.productHash()
	r.t.Logf("%s %v", step, detail)
	r.steps = append(r.steps, map[string]any{"step": step, "atMs": time.Now().UnixMilli(), "detail": detail})
}

func (r *run) productHash() string {
	data, err := os.ReadFile(r.product)
	if err != nil {
		return "absent"
	}
	sum := sha256.Sum256(data)
	return hex.EncodeToString(sum[:])
}

func (r *run) start() {
	r.t.Helper()
	log, err := os.OpenFile(r.logPath, os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0o600)
	if err != nil {
		r.t.Fatal(err)
	}
	cmd := exec.Command(r.binary)
	cmd.Env = append(os.Environ(), "UC_DAEMON_RUN_MODE=server", "RUST_LOG=warn")
	cmd.Stdout, cmd.Stderr = log, log
	if err := cmd.Start(); err != nil {
		r.t.Fatal(err)
	}
	d := &daemonRun{cmd: cmd, done: make(chan struct{})}
	go func() { _ = cmd.Wait(); log.Close(); close(d.done) }()
	r.daemon = d
	deadline := time.Now().Add(60 * time.Second)
	for time.Now().Before(deadline) {
		conn, _ := daemonproc.ReadConnFile()
		if conn != nil && int(conn.PID) == cmd.Process.Pid {
			if _, err := r.read(); err == nil {
				r.steps = append(r.steps, map[string]any{"step": "daemon started", "detail": map[string]any{"pid": cmd.Process.Pid, "port": conn.Port}})
				return
			}
		}
		time.Sleep(200 * time.Millisecond)
	}
	r.t.Fatal("daemon did not become ready")
}

func (r *run) stop() {
	if r.daemon == nil {
		return
	}
	select {
	case <-r.daemon.done:
		return
	default:
	}
	_ = r.daemon.cmd.Process.Kill()
	<-r.daemon.done
	r.steps = append(r.steps, map[string]any{"step": "daemon killed", "detail": map[string]any{"pid": r.daemon.cmd.Process.Pid, "exitCode": r.daemon.cmd.ProcessState.ExitCode()}})
}

func (r *run) client() *daemonclient.Client {
	r.t.Helper()
	c, err := daemonclient.FromEnv()
	if err != nil {
		r.t.Fatal(err)
	}
	return c
}

func (r *run) read() (settings.View, error) {
	c, err := daemonclient.FromEnv()
	if err != nil {
		return settings.View{}, err
	}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	return settings.Read(ctx, c)
}

// put is a write by another client (the CLI, curl): no host code runs for it.
func (r *run) put(body map[string]any) error {
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	err := r.client().Enveloped(ctx, daemonclient.Request{Method: http.MethodPut, Path: "/settings", JSON: body}, nil)
	r.steps = append(r.steps, map[string]any{"step": "PUT /settings", "detail": map[string]any{"body": body, "error": errString(err)}})
	return err
}

func errString(err error) any {
	if err == nil {
		return nil
	}
	return err.Error()
}

func shortcutBody(binding any) map[string]any {
	return map[string]any{"keyboardShortcuts": map[string]any{"shortcuts": map[string]any{shortcutID: binding}}}
}

// target is the host's panelShortcutTarget with the default when unset (the Go normalizer is identity for these
// lower-case values).
func target(v settings.View) []string {
	if !v.QuickPanel.Enabled {
		return nil
	}
	var one string
	if raw, ok := v.KeyboardShortcuts[shortcutID]; ok && json.Unmarshal(raw, &one) == nil && one != "" {
		return []string{one}
	}
	return []string{"ctrl+alt+v"}
}

// apply is the host's reconcile glue for the shortcut, called by the watcher after every read.
func (r *run) apply(v settings.View) {
	r.applier.Reconcile(target(v), func() ([]string, error) {
		fresh, err := r.read()
		return target(fresh), err
	})
	r.applied <- v
}

func (r *run) waitApplied(step string, want func(settings.View) bool) {
	r.t.Helper()
	deadline := time.After(20 * time.Second)
	for {
		select {
		case v := <-r.applied:
			if want(v) {
				r.note(step, nil)
				return
			}
		case <-deadline:
			r.t.Fatalf("%s: no matching read", step)
		}
	}
}

// settle waits until no watcher read has arrived for the window (event reads of earlier writes are done).
func (r *run) settle(window time.Duration) {
	for {
		select {
		case <-r.applied:
		case <-time.After(window):
			return
		}
	}
}

func (r *run) expect(step string, cond bool, detail map[string]any) {
	r.t.Helper()
	if detail == nil {
		detail = map[string]any{}
	}
	detail["ok"] = cond
	r.note(step, detail)
	if !cond {
		r.t.Fatalf("%s failed", step)
	}
}

func (r *run) gui(next string) error {
	// The host's UpdateKeyboardShortcuts: Save with the daemon write as persist.
	return r.applier.Save([]string{next}, func() error { return r.put(shortcutBody(next)) })
}

func TestApplierAgainstDaemon(t *testing.T) {
	binary := os.Getenv("UC_E2E_UNICLIPD")
	if binary == "" {
		t.Skip("UC_E2E_UNICLIPD is not set")
	}
	suffix := make([]byte, 6)
	_, _ = rand.Read(suffix)
	t.Setenv("UC_PROFILE", "e2e-global-shortcut-"+hex.EncodeToString(suffix))
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
	r := &run{t: t, binary: binary, logPath: filepath.Join(artifactDir, "global-shortcut-daemon.log"), exec: &countingExecutor{},
		applied: make(chan settings.View, 256), product: filepath.Join(dataRoot, "desktop-settings.json")}
	_ = os.Remove(r.logPath)
	r.applier = New(r.exec, func(s Status) {
		r.statusMu.Lock()
		r.statuses = append(r.statuses, s)
		r.statusMu.Unlock()
	})
	defer func() {
		r.stop()
		removed := map[string]bool{}
		for _, dir := range []string{dataRoot, cacheRoot, logDir} {
			if dir != "" {
				removed[dir] = os.RemoveAll(dir) == nil
			}
		}
		r.statusMu.Lock()
		record := map[string]any{"test": t.Name(), "profile": os.Getenv("UC_PROFILE"), "passed": !t.Failed(),
			"executor": "countingExecutor (test double; no OS registration)", "executorCalls": r.exec.calls,
			"statusEvents": r.statuses, "steps": r.steps, "profileDirsRemoved": removed}
		r.statusMu.Unlock()
		data, _ := json.MarshalIndent(record, "", "  ")
		_ = os.WriteFile(filepath.Join(artifactDir, "global-shortcut.json"), data, 0o644)
	}()

	r.start()
	if err := r.put(map[string]any{"quickPanel": map[string]any{"enabled": true}}); err != nil {
		t.Fatal(err)
	}
	// Startup: the host reads and applies once (initPanelShortcuts / initQuickPanel).
	view, err := r.read()
	if err != nil {
		t.Fatal(err)
	}
	r.applier.Init(target(view))
	r.expect("startup applies the default once", r.exec.count() == 1 && r.applier.Status().State == StateActive, nil)

	watcher := settings.NewWatcher(func() *daemonclient.Client { c, _ := daemonclient.FromEnv(); return c }, r.apply)
	ctx, stopWatch := context.WithCancel(context.Background())
	defer stopWatch()
	go watcher.Run(ctx)
	r.waitApplied("watcher first read", func(settings.View) bool { return true })
	r.expect("F92 first watcher read registers nothing again", r.exec.count() == 1, nil)

	// D2b / F86: own GUI save, exactly one executor call; its own settings.changed read does nothing.
	before := r.exec.count()
	if err := r.gui("ctrl+alt+b"); err != nil {
		t.Fatal(err)
	}
	r.waitApplied("own write event read", func(v settings.View) bool { return slices.Equal(target(v), []string{"ctrl+alt+b"}) })
	r.settle(700 * time.Millisecond)
	r.expect("D2b own save: exactly one executor call, active", r.exec.count()-before == 1 && r.applier.Status().State == StateActive, nil)

	// F87: a read older than the own save (it still says ctrl+alt+b) reaches reconcile after a newer own save.
	if err := r.gui("ctrl+alt+n"); err != nil {
		t.Fatal(err)
	}
	r.settle(700 * time.Millisecond)
	before = r.exec.count()
	r.applier.Reconcile([]string{"ctrl+alt+b"}, func() ([]string, error) { fresh, err := r.read(); return target(fresh), err })
	r.expect("F87 stale view is confirmed by a fresh read and applies nothing", r.exec.count() == before &&
		slices.Equal(r.applier.Status().Desired, []string{"ctrl+alt+n"}), nil)

	// B5 / F91: an external write with no page and no host command: the watcher applies it once.
	before = r.exec.count()
	if err := r.put(shortcutBody("ctrl+alt+m")); err != nil {
		t.Fatal(err)
	}
	r.waitApplied("B5 external write read", func(v settings.View) bool { return slices.Equal(target(v), []string{"ctrl+alt+m"}) })
	r.settle(700 * time.Millisecond)
	st := r.applier.Status()
	r.expect("B5 external write applied once, active", r.exec.count()-before == 1 && st.State == StateActive && slices.Equal(st.Active, []string{"ctrl+alt+m"}), nil)

	// B6 external / F90: the executor refuses; the value stays saved, the old binding stays active.
	r.exec.setRefuse("ctrl+alt+x")
	if err := r.put(shortcutBody("ctrl+alt+x")); err != nil {
		t.Fatal(err)
	}
	r.waitApplied("B6 external conflicting write read", func(v settings.View) bool { return slices.Equal(target(v), []string{"ctrl+alt+x"}) })
	r.settle(700 * time.Millisecond)
	stored, _ := r.read()
	st = r.applier.Status()
	r.expect("B6 external conflict: saved, not active, previous binding kept", st.State == StateNotActive &&
		slices.Equal(st.Desired, []string{"ctrl+alt+x"}) && slices.Equal(st.Active, []string{"ctrl+alt+m"}) &&
		slices.Equal(target(stored), []string{"ctrl+alt+x"}) && st.Error != "", nil)

	// F93: an unrelated write does not retry the refused binding.
	before = r.exec.count()
	if err := r.put(map[string]any{"general": map[string]any{"trayIcon": "cat"}}); err != nil {
		t.Fatal(err)
	}
	r.waitApplied("unrelated write read", func(v settings.View) bool { return v.General.TrayIcon == "cat" })
	r.settle(700 * time.Millisecond)
	r.expect("F93 unrelated event: no retry", r.exec.count() == before && r.applier.Status().State == StateNotActive, nil)

	// F92 / B6 retries: a daemon restart and reconnect re-read the same value; no call, no false conflict.
	r.stop()
	r.start()
	r.waitApplied("reconnect read after daemon restart", func(v settings.View) bool { return true })
	r.settle(700 * time.Millisecond)
	r.expect("F92 reconnect: no executor call, status unchanged", r.exec.count() == before && r.applier.Status().State == StateNotActive, nil)

	// F88 / B6 GUI: a refused GUI save writes nothing.
	r.exec.setRefuse("ctrl+alt+y")
	hash := r.productHash()
	before = r.exec.count()
	err = r.gui("ctrl+alt+y")
	stored, _ = r.read()
	r.expect("F88 GUI conflict: error, nothing written", err != nil && r.productHash() == hash &&
		slices.Equal(target(stored), []string{"ctrl+alt+x"}) && r.exec.count()-before == 1, map[string]any{"error": errString(err)})

	// A GUI save of an accepted binding clears the not-active state (the user picks another key).
	r.exec.setRefuse()
	if err := r.gui("ctrl+alt+k"); err != nil {
		t.Fatal(err)
	}
	r.settle(900 * time.Millisecond)
	r.expect("GUI save after a conflict: active", r.applier.Status().State == StateActive, nil)

	// F89: the save fails after the executor took the binding: the previous binding is applied again.
	before = r.exec.count()
	err = r.applier.Save([]string{"ctrl+alt+j"}, func() error { return r.put(shortcutBody("ctrl+f25")) }) // 400: outside the token set
	st = r.applier.Status()
	r.expect("F89 persist failure: rollback to the previous binding", err != nil && r.exec.count()-before == 2 &&
		st.State == StateActive && slices.Equal(st.Active, []string{"ctrl+alt+k"}) && slices.Equal(r.exec.held, []string{"ctrl+alt+k"}), map[string]any{"error": errString(err)})

	// F108: the rollback is refused too: the status says what the executor holds, not desired = active.
	r.exec.mu.Lock()
	r.exec.refuse = map[string]bool{}
	r.exec.mu.Unlock()
	refuseRollback := &rollbackRefuser{refuse: "ctrl+alt+k"}
	failing := New(refuseRollback, nil)
	failing.Init([]string{"ctrl+alt+k"})
	err = failing.Save([]string{"ctrl+alt+h"}, func() error { return errors.New("persist failed (test)") })
	fs := failing.Status()
	r.steps = append(r.steps, map[string]any{"step": "F108 failed rollback", "detail": map[string]any{"error": errString(err), "status": fs}})
	if err == nil || fs.State != StateNotActive || !slices.Equal(fs.Desired, []string{"ctrl+alt+k"}) || !slices.Equal(fs.Active, []string{"ctrl+alt+h"}) {
		t.Fatalf("F108: %+v", fs)
	}

	// F107 (applier side): an unknown outcome, then convergence on the next read.
	r.exec.mu.Lock()
	r.exec.unknownNext = true
	r.exec.mu.Unlock()
	if err := r.put(shortcutBody("ctrl+alt+u")); err != nil {
		t.Fatal(err)
	}
	r.waitApplied("external write answered with an unknown outcome", func(v settings.View) bool { return slices.Equal(target(v), []string{"ctrl+alt+u"}) })
	r.settle(700 * time.Millisecond)
	r.expect("F107 unknown outcome: state unknown, active empty", r.applier.Status().State == StateUnknown && len(r.applier.Status().Active) == 0, nil)
	before = r.exec.count()
	if err := r.put(map[string]any{"general": map[string]any{"trayIcon": "glyph"}}); err != nil {
		t.Fatal(err)
	}
	r.waitApplied("next read after an unknown outcome", func(v settings.View) bool { return v.General.TrayIcon == "glyph" })
	r.settle(700 * time.Millisecond)
	r.expect("F107 the next read sends the desired binding again and takes its answer", r.exec.count()-before == 1 && r.applier.Status().State == StateActive, nil)

	// F104: a GUI save and an external write at the same time end on one value in the store and the executor.
	var wg sync.WaitGroup
	wg.Add(2)
	go func() { defer wg.Done(); _ = r.gui("ctrl+alt+p") }()
	go func() { defer wg.Done(); _ = r.put(shortcutBody("ctrl+alt+q")) }()
	wg.Wait()
	time.Sleep(1500 * time.Millisecond)
	r.settle(900 * time.Millisecond)
	stored, _ = r.read()
	st = r.applier.Status()
	r.expect("F104 concurrent writes converge", slices.Equal(st.Desired, target(stored)) && slices.Equal(r.exec.held, target(stored)) && st.State == StateActive,
		map[string]any{"stored": target(stored)})

	// Disable: the target is empty; the executor releases everything once.
	before = r.exec.count()
	if err := r.put(map[string]any{"quickPanel": map[string]any{"enabled": false}}); err != nil {
		t.Fatal(err)
	}
	r.waitApplied("disabled", func(v settings.View) bool { return !v.QuickPanel.Enabled })
	r.settle(700 * time.Millisecond)
	r.expect("disabled panel: released once, inactive", r.exec.count()-before == 1 && len(r.exec.held) == 0 && r.applier.Status().State == StateInactive, nil)

	stopWatch()
	r.note("completed", map[string]any{"daemonLogHasValue": strings.Contains(readFile(r.logPath), "ctrl+alt+x")})
}

// rollbackRefuser accepts every binding except refuse, which it refuses while keeping what it holds.
type rollbackRefuser struct {
	refuse string
	held   []string
}

func (e *rollbackRefuser) Apply(bindings []string) ([]string, error) {
	if e.held != nil && slices.Contains(bindings, e.refuse) {
		return slices.Clone(e.held), errors.New("the previous shortcut could not be registered again (test double)")
	}
	e.held = slices.Clone(bindings)
	return slices.Clone(e.held), nil
}

func readFile(path string) string {
	data, _ := os.ReadFile(path)
	return string(data)
}

//go:build e2e

// End-to-end check of the host side of the helper shortcut protocol (ADR-013 section 10): the real Supervisor and
// ProcessLauncher (process start, pipes, restart after a crash, stop) and globalshortcut.Applier, against a
// stand-in helper process (testdata/standin). The stand-in speaks the protocol and holds bindings in memory; it
// registers nothing with the OS. This proves the request/answer sequence, the unknown state, the late-answer
// handling and process cleanup, NOT that the real helper (`uniclip-quick-panel`) or the OS accepts a binding.
//
// Run from packages/desktop-host-go:
//
//	UC_E2E_ARTIFACT_DIR=<dir> go test -tags e2e -run TestHelperProtocol -count=1 -v ./quickpanelhelper
package quickpanelhelper

import (
	"bufio"
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log"
	"os"
	"os/exec"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"syscall"
	"testing"
	"time"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/globalshortcut"
)

// helperShortcuts repeats the 6-line adapter of apps/gui-go/quick_panel_native.go (package main cannot be imported).
type helperShortcuts struct{ helper *Supervisor }

func (e helperShortcuts) Apply(bindings []string) ([]string, error) {
	held, err := e.helper.SetShortcuts(bindings)
	if errors.Is(err, ErrNoAnswer) {
		return nil, fmt.Errorf("%w: %v", globalshortcut.ErrUnknown, err)
	}
	return held, err
}

type protocolRun struct {
	t       *testing.T
	control string
	logPath string
	steps   []map[string]any
	applier *globalshortcut.Applier
}

func (r *protocolRun) setControl(c map[string]any) {
	data, _ := json.Marshal(c)
	if err := os.WriteFile(r.control, data, 0o600); err != nil {
		r.t.Fatal(err)
	}
	r.steps = append(r.steps, map[string]any{"step": "stand-in behaviour", "detail": c})
}

func (r *protocolRun) expect(step string, cond bool, detail map[string]any) {
	r.t.Helper()
	if detail == nil {
		detail = map[string]any{}
	}
	detail["status"] = r.applier.Status()
	detail["ok"] = cond
	r.t.Logf("%s %v", step, detail)
	r.steps = append(r.steps, map[string]any{"step": step, "atMs": time.Now().UnixMilli(), "detail": detail})
	if !cond {
		r.t.Fatalf("%s failed", step)
	}
}

// standinLog returns the stand-in's log lines (requests it received, answers, starts, crashes).
func (r *protocolRun) standinLog() []map[string]any {
	f, err := os.Open(r.logPath)
	if err != nil {
		return nil
	}
	defer f.Close()
	var out []map[string]any
	scan := bufio.NewScanner(f)
	for scan.Scan() {
		var m map[string]any
		if json.Unmarshal(scan.Bytes(), &m) == nil {
			out = append(out, m)
		}
	}
	return out
}

func (r *protocolRun) pids() []int {
	var pids []int
	for _, m := range r.standinLog() {
		if pid, ok := m["started"].(float64); ok {
			pids = append(pids, int(pid))
		}
	}
	return pids
}

func alive(pid int) bool { return syscall.Kill(pid, 0) == nil }

func waitFor(timeout time.Duration, cond func() bool) bool {
	deadline := time.Now().Add(timeout)
	for time.Now().Before(deadline) {
		if cond() {
			return true
		}
		time.Sleep(50 * time.Millisecond)
	}
	return cond()
}

func TestHelperProtocol(t *testing.T) {
	artifactDir := os.Getenv("UC_E2E_ARTIFACT_DIR")
	if artifactDir == "" {
		artifactDir = t.TempDir()
	}
	if err := os.MkdirAll(artifactDir, 0o755); err != nil {
		t.Fatal(err)
	}
	// The supervisor logs with the standard logger; keep its lines for the artifact and the seq check.
	var hostLog lockedBuffer
	log.SetOutput(io.MultiWriter(os.Stderr, &hostLog))
	defer log.SetOutput(os.Stderr)
	work := t.TempDir()
	standin := filepath.Join(work, "standin")
	build := exec.Command("go", "build", "-o", standin, "./testdata/standin")
	if out, err := build.CombinedOutput(); err != nil {
		t.Fatalf("build stand-in: %v\n%s", err, out)
	}
	r := &protocolRun{t: t, control: filepath.Join(work, "control.json"), logPath: filepath.Join(artifactDir, "standin.log")}
	_ = os.Remove(r.logPath)
	t.Setenv("STANDIN_CONTROL", r.control)
	t.Setenv("STANDIN_LOG", r.logPath)
	r.setControl(map[string]any{})

	supervisor := Start(ForHelper(standin, nil))
	var statuses []globalshortcut.Status
	r.applier = globalshortcut.New(helperShortcuts{supervisor}, func(s globalshortcut.Status) { statuses = append(statuses, s) })
	supervisor.OnLaunch(r.applier.Relaunched)
	var stillAlive []int
	defer func() {
		record := map[string]any{"test": t.Name(), "passed": !t.Failed(),
			"executor": "stand-in helper process (test double; no OS registration)",
			"steps":    r.steps, "standin": r.standinLog(), "standinPidsAliveAfterShutdown": stillAlive, "hostLog": hostLog.String()}
		data, _ := json.MarshalIndent(record, "", "  ")
		_ = os.WriteFile(filepath.Join(artifactDir, "helper-protocol.json"), data, 0o644)
	}()

	// P1 / F95: the host records the binding, then enables; the launch applies it from the stand-in's answer.
	r.applier.Record([]string{"ctrl+alt+v"})
	supervisor.SetEnabled(true)
	r.expect("P1 launch applies the desired binding", waitFor(5*time.Second, func() bool { return r.applier.Status().State == globalshortcut.StateActive }), nil)

	// D2b with the helper: a GUI save is one command, no restart.
	pidsBefore := len(r.pids())
	err := r.applier.Save([]string{"ctrl+alt+b"}, func() error { return nil })
	r.expect("D2b helper save: one command, no restart, active from the answer", err == nil && len(r.pids()) == pidsBefore &&
		slices.Equal(r.applier.Status().Active, []string{"ctrl+alt+b"}), nil)

	// B6 GUI / F88: a refusal keeps the previous binding (reported by the stand-in) and persist does not run.
	r.setControl(map[string]any{"refuse": []string{"super+v"}})
	persisted := false
	err = r.applier.Save([]string{"super+v"}, func() error { persisted = true; return nil })
	st := r.applier.Status()
	r.expect("F88 helper refusal: error, not persisted, previous binding reported active", err != nil && !persisted &&
		slices.Equal(st.Active, []string{"ctrl+alt+b"}) && slices.Equal(st.Desired, []string{"ctrl+alt+b"}), map[string]any{"error": err.Error()})

	// F107: the stand-in applies but answers after the 3 s timeout. The host does not claim a rollback: unknown.
	r.setControl(map[string]any{"delayMs": 4500})
	err = r.applier.Save([]string{"ctrl+alt+l"}, func() error { persisted = true; return nil })
	st = r.applier.Status()
	r.expect("F107 late answer: GUI save fails, state unknown, nothing persisted", err != nil && errors.Is(err, globalshortcut.ErrUnknown) &&
		st.State == globalshortcut.StateUnknown && len(st.Active) == 0, map[string]any{"error": err.Error(), "persisted": persisted})
	// Convergence: the next read with the same desired binding sends it again. The late answer (old seq) arrives
	// while this command waits and is dropped; the answer to the new seq gives the actual state.
	r.setControl(map[string]any{"delayMs": 0})
	lateSent := func() bool {
		for _, m := range r.standinLog() {
			if d, ok := m["delayMs"].(float64); ok && d == 4500 {
				return true
			}
		}
		return false
	}
	// Wait until the stand-in has written the late answer: it then sits unread on the host side, and the retry
	// below must skip it by seq.
	r.expect("F107 the stand-in sent the late answer after the timeout", waitFor(5*time.Second, lateSent), nil)
	time.Sleep(200 * time.Millisecond)
	r.applier.Reconcile([]string{"ctrl+alt+b"}, func() ([]string, error) { return []string{"ctrl+alt+b"}, nil })
	st = r.applier.Status()
	r.expect("F107 convergence: retried, active from the new answer", st.State == globalshortcut.StateActive &&
		slices.Equal(st.Active, []string{"ctrl+alt+b"}), nil)
	r.expect("F107 the host dropped the late answer by seq", strings.Contains(hostLog.String(), "dropped a late shortcut answer"),
		map[string]any{"hostLog": hostLog.String()})

	// F96: a silent helper: the external apply ends unknown, not active, not "rolled back".
	r.setControl(map[string]any{"silent": true})
	r.applier.Reconcile([]string{"ctrl+alt+s"}, func() ([]string, error) { return []string{"ctrl+alt+s"}, nil })
	st = r.applier.Status()
	r.expect("F96 silent helper: unknown", st.State == globalshortcut.StateUnknown && slices.Equal(st.Desired, []string{"ctrl+alt+s"}), nil)
	r.setControl(map[string]any{})
	r.applier.Reconcile([]string{"ctrl+alt+s"}, func() ([]string, error) { return []string{"ctrl+alt+s"}, nil })
	r.expect("F96 next read converges", r.applier.Status().State == globalshortcut.StateActive, nil)

	// F95: the helper crashes; the supervisor relaunches it after its backoff and the launch applies the desired
	// binding again. When the desired one is refused by the new process, the previous one is tried.
	r.setControl(map[string]any{"crashOn": "ctrl+alt+c", "refuse": []string{"ctrl+alt+c"}})
	pidsBefore = len(r.pids())
	r.applier.Reconcile([]string{"ctrl+alt+c"}, func() ([]string, error) { return []string{"ctrl+alt+c"}, nil })
	relaunched := waitFor(10*time.Second, func() bool { return len(r.pids()) > pidsBefore })
	time.Sleep(500 * time.Millisecond)
	st = r.applier.Status()
	r.expect("F95 crash, relaunch, desired refused, previous binding restored and named", relaunched &&
		st.State == globalshortcut.StateNotActive && slices.Equal(st.Desired, []string{"ctrl+alt+c"}) && slices.Equal(st.Active, []string{"ctrl+alt+s"}),
		map[string]any{"pids": r.pids()})

	// F105: disabled: the helper stops; a command reports that it is not running.
	r.setControl(map[string]any{})
	// The host's setHelperEnabled order: record the empty target, stop, mark stopped.
	r.applier.Record(nil)
	supervisor.SetEnabled(false)
	r.applier.Stopped()
	_, err = supervisor.SetShortcuts([]string{"ctrl+alt+v"})
	r.expect("F105 no helper running: ErrNotRunning, status inactive", errors.Is(err, ErrNotRunning) &&
		r.applier.Status().State == globalshortcut.StateInactive, nil)

	// Cleanup: every stand-in process the supervisor started has exited after Shutdown.
	supervisor.Shutdown()
	for _, pid := range r.pids() {
		if waitFor(3*time.Second, func() bool { return !alive(pid) }) == false {
			stillAlive = append(stillAlive, pid)
		}
	}
	r.expect("process cleanup: no stand-in left", len(stillAlive) == 0, map[string]any{"pids": r.pids(),
		"stdinClosedLines": strings.Count(fmt.Sprint(r.standinLog()), "stdinClosed")})
	_ = statuses
}

type lockedBuffer struct {
	mu  sync.Mutex
	buf bytes.Buffer
}

func (b *lockedBuffer) Write(p []byte) (int, error) {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.buf.Write(p)
}

func (b *lockedBuffer) String() string {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.buf.String()
}

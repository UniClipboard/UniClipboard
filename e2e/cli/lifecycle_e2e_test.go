package e2e

// Process acceptance for 0021: the CLI acquires, shares and retires unicd on demand.
// Every scenario drives real unic and unicd binaries built with the unic_e2e tag. The tag
// adds only fixture controls (slow start, retiring hold, shortened windows); it changes no
// lifecycle logic. Run with `make service-lifecycle-e2e`.

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"syscall"
	"testing"
	"time"

	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/internal/rendezvous/rvtest"
	"github.com/uniclipboard/unic/internal/servicelifecycle"
	"github.com/uniclipboard/unic/internal/v1migration/v1fixture"
)

// lifecycleScenarios lists every scenario this suite must report. A missing or skipped one fails the run.
var lifecycleScenarios = []string{
	"C01", "C02", "C03", "C04", "C05", "C06", "C07", "C08", "C09", "C10", "C11", "C12", "C13", "C14", "C15",
	"C16", "C17", "C18", "C19", "C19b", "C20", "C25", "C27", "C28", "C29", "C30", "C31", "C32", "C33", "C35",
}

// notRun records scenarios that this suite deliberately does not exercise, with the reason.
var notRun = map[string]string{
	"C18b": "not run by this suite; asserted by TestStartupProcesses (make desktop-startup-e2e): a lightweight launcher refuses a CLI-temporary generation",
	"C21":  "not run by this suite; asserted by TestCLI/cold-start-send",
	"C22":  "not run by this suite; asserted by TestCLI/cold-start-offline-peer",
	"C23":  "not run by this suite; asserted by TestCLI/send-skips-offline-target, TestSendFaults (target fails mid-body: exit 4, outcome failed) and TestCLI/send-slow-peer-timeout (frozen peer: outcome timeout, one commit, no replay)",
	"C24":  "not run by this suite; TestCLI/cold-start-send-response-lost asserts a killed CLI leaves one commit and no replay; TestCLI/service-replaced-mid-submit asserts a killed and restarted service leaves one commit, exit 1 with resource_expired, and no replay",
	"C26":  "not run by this suite; asserted by TestCLI/cold-start-pairing and TestCLI/cold-start-pairing-outcomes",
	"C34":  "not run by this suite; asserted by TestPostHogTemporaryRestarts (make posthog-e2e)",
	"C36":  "only the host platform runs; other platforms are compile-checked, not process-tested",
}

type recorder struct {
	mu         sync.Mutex
	start      time.Time
	timeline   []map[string]any
	results    map[string]map[string]any
	commands   []map[string]any
	cleanup    []map[string]any
	operations []map[string]any
	measures   map[string]any
}

func (r *recorder) event(label string, fields map[string]any) {
	r.mu.Lock()
	defer r.mu.Unlock()
	e := map[string]any{"t_ms": time.Since(r.start).Milliseconds(), "event": label}
	for k, v := range fields {
		e[k] = v
	}
	r.timeline = append(r.timeline, e)
}

func (r *recorder) result(id, status, detail string) {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.results[id] = map[string]any{"status": status, "detail": detail}
}

// lab is one isolated profile with its own socket.
type lab struct {
	t    *testing.T
	rec  *recorder
	name string
	home string
	sock string
	env  []string
}

func newLab(t *testing.T, rec *recorder, root, name string, extra ...string) *lab {
	home := filepath.Join(root, name)
	if err := os.MkdirAll(home, 0o700); err != nil {
		t.Fatal(err)
	}
	l := &lab{t: t, rec: rec, name: name, home: home, sock: filepath.Join(root, name+".sock")}
	l.env = append(os.Environ(), "UNIC_HOME="+home, "UNIC_SOCK="+l.sock, "UNIC_RENDEZVOUS_URL=http://127.0.0.1:1")
	l.env = append(l.env, extra...)
	if err := os.WriteFile(filepath.Join(home, "config.toml"), []byte("\n[telemetry]\nenabled=false\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	return l
}

func (l *lab) with(extra ...string) *lab {
	c := *l
	c.env = append(slices.Clone(l.env), extra...)
	return &c
}

// proc is a background process with captured output.
type proc struct {
	cmd  *exec.Cmd
	out  syncBuffer
	err  syncBuffer
	done chan struct{}
	code int
}

type syncBuffer struct {
	mu sync.Mutex
	b  bytes.Buffer
}

func (s *syncBuffer) Write(p []byte) (int, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.b.Write(p)
}
func (s *syncBuffer) String() string { s.mu.Lock(); defer s.mu.Unlock(); return s.b.String() }

func (l *lab) spawn(binary string, stdin io.Reader, args ...string) *proc {
	l.t.Helper()
	cmd := exec.Command(filepath.Join(binDir, binary), args...)
	cmd.Env = l.env
	cmd.Stdin = stdin
	p := &proc{cmd: cmd, done: make(chan struct{})}
	cmd.Stdout, cmd.Stderr = &p.out, &p.err
	if err := cmd.Start(); err != nil {
		l.t.Fatal(err)
	}
	l.rec.mu.Lock()
	l.rec.commands = append(l.rec.commands, map[string]any{"binary": binary, "args": args, "lab": l.name})
	l.rec.mu.Unlock()
	go func() {
		err := cmd.Wait()
		p.code = 0
		var exit *exec.ExitError
		if errors.As(err, &exit) {
			p.code = exit.ExitCode()
		} else if err != nil {
			p.code = -1
		}
		close(p.done)
	}()
	l.t.Cleanup(func() { _ = cmd.Process.Kill() })
	return p
}

func (l *lab) unic(args ...string) *proc { return l.spawn("unic", nil, args...) }

// run waits for a foreground command.
func (l *lab) run(args ...string) (stdout, stderr string, code int) {
	l.t.Helper()
	p := l.unic(args...)
	select {
	case <-p.done:
	case <-time.After(90 * time.Second):
		_ = p.cmd.Process.Kill()
		l.t.Fatalf("unic %v did not finish", args)
	}
	return p.out.String(), p.err.String(), p.code
}

func (p *proc) wait(t *testing.T, within time.Duration) {
	t.Helper()
	select {
	case <-p.done:
	case <-time.After(within):
		t.Fatalf("process did not exit within %s; stderr=%q", within, p.err.String())
	}
}

func (p *proc) exited() bool {
	select {
	case <-p.done:
		return true
	default:
		return false
	}
}

func (l *lab) instance() (servicelifecycle.Instance, error) {
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	return api.NewClient(l.sock).Service(ctx)
}

// waitInstance polls the private socket until the condition holds.
func (l *lab) waitInstance(within time.Duration, ok func(servicelifecycle.Instance) bool) servicelifecycle.Instance {
	l.t.Helper()
	deadline := time.Now().Add(within)
	for time.Now().Before(deadline) {
		if v, err := l.instance(); err == nil && ok(v) {
			return v
		}
		time.Sleep(50 * time.Millisecond)
	}
	l.t.Fatalf("service condition not reached within %s", within)
	return servicelifecycle.Instance{}
}

func alive(pid int) bool { return syscall.Kill(pid, 0) == nil }

// assertStopped checks process termination, socket removal and lock release.
func (l *lab) assertStopped(pid int, within time.Duration) {
	l.t.Helper()
	deadline := time.Now().Add(within)
	for alive(pid) && time.Now().Before(deadline) {
		time.Sleep(50 * time.Millisecond)
	}
	if alive(pid) {
		l.t.Fatalf("unicd %d still running after %s", pid, within)
	}
	if _, err := os.Stat(l.sock); err == nil {
		l.t.Fatalf("socket still present after service stopped")
	}
	// The next launcher can take the transition lock immediately.
	lock := make(chan struct{})
	go func() {
		f, err := os.OpenFile(filepath.Join(l.home, "service-transition.lock"), os.O_CREATE|os.O_RDWR, 0o600)
		if err == nil {
			if syscall.Flock(int(f.Fd()), syscall.LOCK_EX) == nil {
				_ = syscall.Flock(int(f.Fd()), syscall.LOCK_UN)
			}
			_ = f.Close()
		}
		close(lock)
	}()
	select {
	case <-lock:
	case <-time.After(3 * time.Second):
		l.t.Fatalf("transition lock still held")
	}
	l.rec.mu.Lock()
	l.rec.cleanup = append(l.rec.cleanup, map[string]any{"lab": l.name, "process_gone": true, "socket_removed": true, "lock_reacquired": true})
	l.rec.mu.Unlock()
}

func (l *lab) assertNoService() {
	l.t.Helper()
	if _, err := os.Stat(l.sock); err == nil {
		l.t.Fatalf("a service socket exists; no service was expected")
	}
	if _, err := os.Stat(filepath.Join(l.home, "cli-daemon.log")); err == nil {
		l.t.Fatalf("a temporary service was launched; none was expected")
	}
}

// standalone starts a plain unicd and waits for it.
func (l *lab) standalone() (*proc, servicelifecycle.Instance) {
	p := l.spawn("unicd", nil)
	return p, l.waitInstance(20*time.Second, func(v servicelifecycle.Instance) bool { return v.Mode == servicelifecycle.ModeStandalone })
}

func (l *lab) signal(p *proc, s syscall.Signal) { _ = p.cmd.Process.Signal(s) }

func unicdCount() int {
	out, _ := exec.Command("pgrep", "-f", filepath.Join(binDir, "unicd")).Output()
	return len(strings.Fields(string(out)))
}

func TestServiceLifecycle(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1 to run the lifecycle process acceptance")
	}
	artifacts := os.Getenv("UNIC_E2E_ARTIFACTS")
	if artifacts == "" {
		artifacts = t.TempDir()
	}
	if err := os.MkdirAll(artifacts, 0o700); err != nil {
		t.Fatal(err)
	}
	root := shortTemp(t)
	rec := &recorder{start: time.Now(), results: map[string]map[string]any{}, measures: map[string]any{}}

	scenario := func(id string, fn func(t *testing.T)) {
		ok := t.Run(id, func(t *testing.T) {
			rec.event("begin", map[string]any{"scenario": id})
			fn(t)
		})
		status := "PASS"
		if !ok {
			status = "FAIL"
		}
		rec.result(id, status, "")
	}

	scenario("C01", func(t *testing.T) {
		l := newLab(t, rec, root, "c01")
		for _, args := range [][]string{
			{"help"}, {"help", "--json"}, {"config", "path"}, {"plugin", "list"}, {"dev", "logs", "help"}, {"history", "help"},
			{"nosuch"}, {"dev", "diagnostics", "bad"}, {"history"}, {"history", "bogus"}, {"dev", "logs"}, {"device", "remove"}, {"send", "-f"}, {"device", "approve", "x"}, {"dev", "submission"}, {"device"}, {"dev"}, {"logs"}, {"copy"},
			// A source that is neither a 6-digit code nor a link never reaches the service.
			{"recv", "hello"}, {"recv", "12345"}, {"recv", "tcabcdef"}, {"recv", "unic-pair:abc"},
		} {
			l.run(args...)
			l.assertNoService()
		}
	})

	scenario("C02", func(t *testing.T) {
		l := newLab(t, rec, root, "c02")
		out, _, code := l.run("help", "--json")
		if code != 0 {
			t.Fatalf("help --json exit %d", code)
		}
		var leaves []struct {
			Leaf        string `json:"leaf"`
			Requirement string `json:"requirement"`
			Completion  string `json:"completion"`
		}
		if err := json.Unmarshal([]byte(out), &leaves); err != nil {
			t.Fatal(err)
		}
		names := map[string]bool{}
		for _, v := range leaves {
			names[v.Leaf] = true
			if !slices.Contains([]string{"local", "service", "service+clipboard", "offline"}, v.Requirement) || v.Completion == "" {
				t.Fatalf("leaf %q has no explicit declaration: %+v", v.Leaf, v)
			}
		}
		// Every command word printed by help must resolve to a declared leaf.
		help, _, _ := l.run("help")
		for line := range strings.SplitSeq(help, "\n") {
			if !strings.HasPrefix(line, "  ") || strings.TrimSpace(line) == "" {
				continue
			}
			word := strings.Trim(strings.Fields(line)[0], ",")
			found := false
			for n := range names {
				if n == word || strings.HasPrefix(n, word+" ") {
					found = true
				}
			}
			if !found {
				t.Fatalf("help lists %q but no leaf declares it", word)
			}
		}
		// And every declared top-level command must be printed, so a new leaf cannot ship undocumented.
		for n := range names {
			word := strings.Fields(n)[0]
			if word == "help" {
				continue
			}
			printed := false
			for line := range strings.SplitSeq(help, "\n") {
				if strings.HasPrefix(line, "  ") && slices.Contains(strings.FieldsFunc(line, func(r rune) bool { return r == ' ' || r == ',' || r == '(' || r == ')' || r == '|' }), word) {
					printed = true
				}
			}
			if !printed {
				t.Fatalf("leaf %q is declared but help does not print %q", n, word)
			}
		}
		for _, req := range []string{`"service"`, `"local"`, `"service+clipboard"`, `"offline"`} {
			if !strings.Contains(out, req) {
				t.Fatalf("catalog lacks requirement %s", req)
			}
		}
		_ = os.WriteFile(filepath.Join(artifacts, "command-catalog.json"), []byte(out), 0o600)
		l.assertNoService()
	})

	scenario("C03", func(t *testing.T) {
		l := newLab(t, rec, root, "c03")
		began := time.Now()
		out, _, code := l.run("status")
		rec.measures["cold_status_ms"] = time.Since(began).Milliseconds()
		if code != 0 || !strings.Contains(out, "device") {
			t.Fatalf("cold status failed: %d %q", code, out)
		}
		if _, err := os.Stat(l.sock); err == nil {
			t.Fatal("temporary service outlived its last user")
		}
		if n := unicdCount(); n != 0 {
			t.Fatalf("%d unicd processes remain", n)
		}
		// The override window holds the temporary generation; observe it.
		p := l.unic("dev", "logs", "level", "debug", "--for", "3s")
		inst := l.waitInstance(20*time.Second, func(v servicelifecycle.Instance) bool {
			return v.Mode == servicelifecycle.ModeCLITemporary && v.Leases == 1
		})
		p.wait(t, 20*time.Second)
		if p.code != 0 || !strings.Contains(p.err.String(), "keeping the temporary service") {
			t.Fatalf("override command: %d %q", p.code, p.err.String())
		}
		l.assertStopped(inst.PID, 15*time.Second)
	})

	scenario("C04", func(t *testing.T) {
		l := newLab(t, rec, root, "c04")
		sa, inst := l.standalone()
		warm := time.Now()
		if out, _, code := l.run("status"); code != 0 || !strings.Contains(out, "device") {
			t.Fatal("warm status failed")
		}
		rec.measures["warm_status_ms"] = time.Since(warm).Milliseconds()
		if _, _, code := l.run("history", "show", "missing"); code == 0 {
			t.Fatal("expected a history error")
		}
		w := l.unic("dev", "watch")
		l.waitInstance(10*time.Second, func(v servicelifecycle.Instance) bool { return v.Leases == 1 })
		l.signal(w, syscall.SIGINT)
		w.wait(t, 10*time.Second)
		after := l.waitInstance(5*time.Second, func(v servicelifecycle.Instance) bool { return v.Leases == 0 })
		if after.ID != inst.ID || !alive(inst.PID) {
			t.Fatal("standalone service changed or stopped")
		}
		l.signal(sa, syscall.SIGTERM)
		sa.wait(t, 15*time.Second)
		// Desktop-owned: the parent pipe stays open for the whole check.
		pr, pw := io.Pipe()
		d := l.spawn("unicd", pr, "--desktop-owned")
		di := l.waitInstance(20*time.Second, func(v servicelifecycle.Instance) bool { return v.Mode == servicelifecycle.ModeDesktop })
		if _, _, code := l.run("status"); code != 0 {
			t.Fatal("status on desktop-owned service failed")
		}
		if v, err := l.instance(); err != nil || v.ID != di.ID || v.Leases != 0 {
			t.Fatalf("desktop-owned service changed: %+v %v", v, err)
		}
		_ = pw.Close()
		d.wait(t, 15*time.Second)
	})

	scenario("C05", func(t *testing.T) {
		l := newLab(t, rec, root, "c05")
		a, b := l.unic("dev", "watch"), l.unic("dev", "watch")
		inst := l.waitInstance(30*time.Second, func(v servicelifecycle.Instance) bool { return v.Leases == 2 })
		if inst.Mode != servicelifecycle.ModeCLITemporary || unicdCount() != 1 {
			t.Fatalf("expected one temporary generation, got %+v with %d processes", inst, unicdCount())
		}
		// C25: a third command is not blocked by the two long-lived streams.
		began := time.Now()
		if _, _, code := l.run("status"); code != 0 || time.Since(began) > 5*time.Second {
			t.Fatal("a concurrent command was blocked or failed")
		}
		l.signal(a, syscall.SIGINT)
		l.signal(b, syscall.SIGINT)
		a.wait(t, 15*time.Second)
		b.wait(t, 15*time.Second)
		l.assertStopped(inst.PID, 15*time.Second)
	})

	scenario("C06", func(t *testing.T) {
		l := newLab(t, rec, root, "c06")
		starter := l.unic("dev", "watch")
		inst := l.waitInstance(30*time.Second, func(v servicelifecycle.Instance) bool { return v.Leases == 1 })
		borrower := l.unic("dev", "watch")
		l.waitInstance(10*time.Second, func(v servicelifecycle.Instance) bool { return v.Leases == 2 })
		// C09: the starter dies without cleanup while the borrower is active.
		l.signal(starter, syscall.SIGKILL)
		starter.wait(t, 5*time.Second)
		after := l.waitInstance(10*time.Second, func(v servicelifecycle.Instance) bool { return v.Leases == 1 })
		if after.ID != inst.ID || !alive(inst.PID) || borrower.exited() {
			t.Fatal("the borrower lost its service when the starter died")
		}
		// C10: the last user dies too; no idle retention.
		killed := time.Now()
		l.signal(borrower, syscall.SIGKILL)
		borrower.wait(t, 5*time.Second)
		l.assertStopped(inst.PID, 15*time.Second)
		rec.measures["last_user_kill_to_stop_ms"] = time.Since(killed).Milliseconds()
	})

	scenario("C09", func(t *testing.T) {
		l := newLab(t, rec, root, "c09")
		only := l.unic("dev", "watch")
		inst := l.waitInstance(30*time.Second, func(v servicelifecycle.Instance) bool { return v.Leases == 1 })
		l.signal(only, syscall.SIGKILL)
		only.wait(t, 5*time.Second)
		l.assertStopped(inst.PID, 15*time.Second)

		// A command that holds a bounded session dies the same way: its lease is reclaimed.
		held := newLab(t, rec, root, "c09s")
		session := held.unic("dev", "logs", "level", "debug", "--for", "60s")
		sessionInst := held.waitInstance(30*time.Second, func(v servicelifecycle.Instance) bool { return v.Leases == 1 })
		held.signal(session, syscall.SIGKILL)
		session.wait(t, 5*time.Second)
		held.assertStopped(sessionInst.PID, 15*time.Second)
	})

	scenario("C10", func(t *testing.T) {
		l := newLab(t, rec, root, "c10")
		if _, _, code := l.run("status"); code != 0 {
			t.Fatal("status failed")
		}
		time.Sleep(500 * time.Millisecond)
		if n := unicdCount(); n != 0 {
			t.Fatalf("a finished command left %d unicd processes", n)
		}
		l.assertNoServiceSocket()
	})

	scenario("C07", func(t *testing.T) {
		// The retiring generation answers for three seconds, so a new command meets it deterministically.
		l := newLab(t, rec, root, "c07", "UNIC_E2E_RETIRE_HOLD_MS=3000")
		holder := l.unic("dev", "watch")
		first := l.waitInstance(30*time.Second, func(v servicelifecycle.Instance) bool { return v.Leases == 1 })
		l.signal(holder, syscall.SIGINT)
		l.waitInstance(10*time.Second, func(v servicelifecycle.Instance) bool { return v.Retiring })
		out, _, code := l.run("status")
		if code != 0 || !strings.Contains(out, "device") {
			t.Fatalf("a command arriving during retirement failed: %d %q", code, out)
		}
		holder.wait(t, 20*time.Second)
		if alive(first.PID) {
			t.Fatal("the retiring generation did not stop")
		}
		rec.event("retiring generation replaced", map[string]any{"scenario": "C07"})
		time.Sleep(4 * time.Second)
		if n := unicdCount(); n != 0 {
			t.Fatalf("%d unicd remain after both generations finished", n)
		}
	})

	scenario("C08", func(t *testing.T) {
		l := newLab(t, rec, root, "c08", "UNIC_E2E_START_DELAY_MS=4000")
		p := l.unic("status")
		time.Sleep(800 * time.Millisecond)
		l.signal(p, syscall.SIGKILL)
		p.wait(t, 5*time.Second)
		deadline := time.Now().Add(15 * time.Second)
		for unicdCount() != 0 && time.Now().Before(deadline) {
			time.Sleep(100 * time.Millisecond)
		}
		if unicdCount() != 0 {
			t.Fatal("the initial owner's death left a service running")
		}
		l.assertNoServiceSocket()
	})

	scenario("C11", func(t *testing.T) {
		l := newLab(t, rec, root, "c11")
		lone := filepath.Join(root, "lone")
		_ = os.MkdirAll(lone, 0o700)
		data, err := os.ReadFile(filepath.Join(binDir, "unic"))
		if err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(lone, "unic"), data, 0o755); err != nil {
			t.Fatal(err)
		}
		// binDir stays on PATH: a PATH fallback would find a working unicd.
		cmd := exec.Command(filepath.Join(lone, "unic"), "status")
		cmd.Env = append(slices.Clone(l.env), "PATH="+binDir+":"+os.Getenv("PATH"))
		var stderr bytes.Buffer
		cmd.Stderr = &stderr
		if err := cmd.Run(); err == nil || !strings.Contains(stderr.String(), "unicd") {
			t.Fatalf("expected a clear missing-unicd error, got %v %q", err, stderr.String())
		}
		l.assertNoServiceSocket()
	})

	scenario("C12", func(t *testing.T) {
		l := newLab(t, rec, root, "c12")
		_ = os.WriteFile(filepath.Join(l.home, "config.toml"), []byte("not = [valid"), 0o600)
		before := dirNames(l.home)
		_, stderr, code := l.run("status")
		if code != 1 || !strings.Contains(stderr, "error:") {
			t.Fatalf("expected a startup failure, got %d %q", code, stderr)
		}
		l.assertNoServiceSocket()
		for _, n := range dirNames(l.home) {
			if !slices.Contains(before, n) && n != "cli-daemon.log" && n != "service-transition.lock" {
				t.Fatalf("startup failure created %q", n)
			}
		}
		if data, _ := os.ReadFile(filepath.Join(l.home, "config.toml")); string(data) != "not = [valid" {
			t.Fatal("startup failure rewrote the configuration")
		}
	})

	scenario("C13", func(t *testing.T) {
		l := newLab(t, rec, root, "c13")
		owner, _ := l.standalone()
		// The socket vanishes while the Engine lock stays held: the typed in-use exit is retried.
		_ = os.Remove(l.sock)
		p := l.unic("status")
		time.Sleep(2 * time.Second)
		if !alive(owner.cmd.Process.Pid) || p.exited() {
			t.Fatal("the launcher stopped the owner or gave up early")
		}
		l.signal(owner, syscall.SIGKILL)
		p.wait(t, 40*time.Second)
		if p.code != 0 || !strings.Contains(p.out.String(), "device") {
			t.Fatalf("status after the lock was released: %d %q %q", p.code, p.out.String(), p.err.String())
		}
	})

	scenario("C14", func(t *testing.T) {
		l := newLab(t, rec, root, "c14")
		for name, handler := range map[string]http.HandlerFunc{
			"silent": func(w http.ResponseWriter, r *http.Request) { time.Sleep(5 * time.Second) },
			"wrong-profile": func(w http.ResponseWriter, r *http.Request) {
				_ = json.NewEncoder(w).Encode(servicelifecycle.Instance{ID: "x", Directory: "/other", Mode: servicelifecycle.ModeStandalone})
			},
		} {
			ln, err := net.Listen("unix", l.sock)
			if err != nil {
				t.Fatal(err)
			}
			srv := &http.Server{Handler: handler}
			go func() { _ = srv.Serve(ln) }()
			_, stderr, code := l.run("status")
			if code != 1 || !(strings.Contains(stderr, "inspect existing unicd") || strings.Contains(stderr, "different state directory")) {
				t.Fatalf("%s: unexpected result %d %q", name, code, stderr)
			}
			if _, err := os.Stat(l.sock); err != nil {
				t.Fatalf("%s: the foreign socket was unlinked", name)
			}
			if _, err := os.Stat(filepath.Join(l.home, "cli-daemon.log")); err == nil {
				t.Fatalf("%s: a service was launched over a foreign listener", name)
			}
			_ = srv.Close()
			_ = os.Remove(l.sock)
		}
	})

	scenario("C15", func(t *testing.T) {
		l := newLab(t, rec, root, "c15")
		owner, first := l.standalone()
		l.signal(owner, syscall.SIGKILL)
		owner.wait(t, 5*time.Second)
		if _, err := os.Stat(l.sock); err != nil {
			t.Fatal("a killed service left no stale socket; the recovery path was not exercised")
		}
		out, _, code := l.run("status")
		if code != 0 || !strings.Contains(out, "device") {
			t.Fatalf("status over a stale socket: %d", code)
		}
		if unicdCount() != 0 || first.PID == 0 {
			t.Fatal("unexpected process state")
		}
	})

	// C16 asserts both halves: the old command reports the interruption (C17) and its cleanup
	// leaves the replacement alone (C16). The receipt lists C17 with C16's result.
	scenario("C16", func(t *testing.T) {
		// A normal desktop replaces the temporary generation by stopping the observed generation first.
		l := newLab(t, rec, root, "c16")
		watcher := l.unic("dev", "watch")
		old := l.waitInstance(30*time.Second, func(v servicelifecycle.Instance) bool { return v.Leases == 1 })
		ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
		defer cancel()
		if err := api.NewClient(l.sock).StopService(ctx, old.ID); err != nil {
			t.Fatal(err)
		}
		deadline := time.Now().Add(15 * time.Second)
		for time.Now().Before(deadline) {
			if _, err := os.Stat(l.sock); err != nil {
				break
			}
			time.Sleep(50 * time.Millisecond)
		}
		pr, pw := io.Pipe()
		desktop := l.spawn("unicd", pr, "--desktop-owned")
		replacement := l.waitInstance(20*time.Second, func(v servicelifecycle.Instance) bool { return v.Mode == servicelifecycle.ModeDesktop })
		// C17: the affected command reports the interruption.
		watcher.wait(t, 20*time.Second)
		if watcher.code != 4 || !strings.Contains(watcher.err.String(), "stopped or was replaced") {
			t.Fatalf("watch reported %d %q", watcher.code, watcher.err.String())
		}
		// C16: the old command's cleanup must not touch the replacement.
		if v, err := l.instance(); err != nil || v.ID != replacement.ID || !alive(replacement.PID) {
			t.Fatalf("the replacement was disturbed: %+v %v", v, err)
		}
		_ = pw.Close()
		desktop.wait(t, 15*time.Second)
	})

	scenario("C18", func(t *testing.T) {
		l := newLab(t, rec, root, "c18")
		holder := l.unic("dev", "watch")
		inst := l.waitInstance(30*time.Second, func(v servicelifecycle.Instance) bool { return v.Leases == 1 })
		direct := l.spawn("unicd", nil)
		direct.wait(t, 15*time.Second)
		if direct.code == 0 {
			t.Fatal("a direct standalone start took over a temporary generation")
		}
		if v, err := l.instance(); err != nil || v.ID != inst.ID || v.Leases != 1 {
			t.Fatal("the temporary generation was disturbed")
		}
		l.signal(holder, syscall.SIGINT)
		holder.wait(t, 15*time.Second)
	})

	scenario("C19", func(t *testing.T) {
		l := newLab(t, rec, root, "c19")
		owner, inst := l.standalone()
		_, _, code := l.run("migrate", "--source", filepath.Join(root, "no-such-source"), "--target", l.home)
		if code == 0 {
			t.Fatal("migration of a missing source succeeded")
		}
		if v, err := l.instance(); err != nil || v.ID != inst.ID || !alive(owner.cmd.Process.Pid) {
			t.Fatal("migration stopped or replaced the running owner")
		}
		l.signal(owner, syscall.SIGTERM)
		owner.wait(t, 15*time.Second)
	})

	scenario("C19b", func(t *testing.T) {
		// A valid v1 source reaches the target lock. A running owner makes migration fail without any service action.
		l := newLab(t, rec, root, "c19b")
		source, _, fixture, err := v1fixture.Build(filepath.Join(root, "c19b-source"))
		if err != nil {
			t.Fatal(err)
		}
		owner, inst := l.standalone()
		migrate := func() *proc {
			p := l.spawn("unic", strings.NewReader(fixture.Passphrase+"\n"), "migrate", "--source", source, "--target", l.home, "--passphrase-stdin", "--json")
			p.wait(t, 60*time.Second)
			return p
		}
		blocked := migrate()
		if blocked.code == 0 || blocked.code == 3 {
			t.Fatalf("migration into a running owner's profile succeeded: %d %s", blocked.code, blocked.out.String())
		}
		if !strings.Contains(blocked.err.String(), "in_use") {
			t.Fatalf("expected an in-use error, got %q", blocked.err.String())
		}
		if v, err := l.instance(); err != nil || v.ID != inst.ID || v.Leases != 0 || !alive(owner.cmd.Process.Pid) || unicdCount() != 1 {
			t.Fatalf("migration changed the running owner: %+v %v", v, err)
		}
		// Once the owner is gone, the same command and source succeed. The block came from the lock, not the source.
		l.signal(owner, syscall.SIGTERM)
		owner.wait(t, 15*time.Second)
		l.assertNoService()
		done := migrate()
		if done.code != 0 && done.code != 3 {
			t.Fatalf("migration into a free profile failed: %d %s", done.code, done.err.String())
		}
		l.assertNoService()
	})

	scenario("C20", func(t *testing.T) {
		l := newLab(t, rec, root, "c20")
		for _, args := range [][]string{{"dev", "paste"}, {"dev", "copy", "hello"}} {
			_, stderr, code := l.run(args...)
			if code != 1 || stderr == "" {
				t.Fatalf("%v without an adapter: %d %q", args, code, stderr)
			}
			l.assertNoServiceSocket()
		}
	})

	rv := rvtest.New()
	t.Cleanup(rv.Close)

	scenario("C25", func(t *testing.T) {
		l := newLab(t, rec, root, "c25")
		stream := l.unic("dev", "watch")
		inst := l.waitInstance(30*time.Second, func(v servicelifecycle.Instance) bool { return v.Leases == 1 })
		// The stream holds a lease but not the transition lock: a new command is not delayed.
		began := time.Now()
		if _, _, code := l.run("status"); code != 0 || time.Since(began) > 5*time.Second {
			t.Fatalf("a command waited behind a long stream: %d after %s", code, time.Since(began))
		}
		l.signal(stream, syscall.SIGINT)
		stream.wait(t, 15*time.Second)
		l.assertStopped(inst.PID, 15*time.Second)
	})

	scenario("C27", func(t *testing.T) {
		l := newLab(t, rec, root, "c27", "UNIC_RENDEZVOUS_URL="+rv.URL)
		code := l.unic("device", "add")
		inst := l.waitInstance(30*time.Second, func(v servicelifecycle.Instance) bool { return v.Leases == 1 })
		// The invitation is printed, yet the temporary service must stay up.
		deadline := time.Now().Add(10 * time.Second)
		for !strings.Contains(code.out.String(), "Valid until") && time.Now().Before(deadline) {
			time.Sleep(50 * time.Millisecond)
		}
		if code.exited() || !alive(inst.PID) {
			t.Fatal("the temporary service stopped at the printed invitation")
		}
		if !strings.Contains(code.err.String(), "keeping the temporary service running") {
			t.Fatalf("the foreground duration was not announced: %q", code.err.String())
		}
		l.signal(code, syscall.SIGINT)
		code.wait(t, 15*time.Second)
		l.assertStopped(inst.PID, 15*time.Second)
		// A persistent owner returns after publication.
		_, _ = l.standalone()
		began := time.Now()
		if _, _, c := l.run("device", "add"); c != 0 || time.Since(began) > 10*time.Second {
			t.Fatal("code did not return under a persistent owner")
		}
	})

	scenario("C28", func(t *testing.T) {
		l := newLab(t, rec, root, "c28", "UNIC_RENDEZVOUS_URL="+rv.URL)
		a := l.unic("device", "add")
		l.waitInstance(30*time.Second, func(v servicelifecycle.Instance) bool { return v.Leases == 1 })
		deadline := time.Now().Add(10 * time.Second)
		for !strings.Contains(a.out.String(), "Valid until") && time.Now().Before(deadline) {
			time.Sleep(50 * time.Millisecond)
		}
		first, err := api.NewClient(l.sock).Code(context.Background())
		if err != nil {
			t.Fatal(err)
		}
		// Invitation B replaces A. A must end on its own without cancelling B.
		second, err := api.NewClient(l.sock).NewCode(context.Background())
		if err != nil || second.Code == first.Code {
			t.Fatalf("no replacement invitation: %v", err)
		}
		b := l.unic("device", "add") // yet another replacement, held by B
		a.wait(t, 15*time.Second)
		if still, err := api.NewClient(l.sock).Code(context.Background()); err != nil || still.Code == "" {
			t.Fatalf("an older command's exit withdrew the newer invitation: %v", err)
		}
		l.signal(b, syscall.SIGINT)
		b.wait(t, 15*time.Second)
	})

	scenario("C29", func(t *testing.T) {
		l := newLab(t, rec, root, "c29")
		a := l.unic("dev", "logs", "level", "debug", "--for", "30s")
		inst := l.waitInstance(30*time.Second, func(v servicelifecycle.Instance) bool { return v.Leases == 1 })
		// A newer override replaces A's; A ends without resetting it.
		if _, err := api.NewClient(l.sock).SetLogLevel(context.Background(), "local", "warn", 20); err != nil {
			t.Fatal(err)
		}
		a.wait(t, 15*time.Second)
		state, err := api.NewClient(l.sock).LogLevels(context.Background())
		if err != nil || state.Local.Effective != "warn" {
			// the service may already be retiring after A's release; the check is only meaningful while it answers
			if _, e := l.instance(); e == nil {
				t.Fatalf("the newer override was removed: %+v %v", state.Local, err)
			}
		}
		l.assertStopped(inst.PID, 15*time.Second)
	})

	scenario("C30", func(t *testing.T) {
		l := newLab(t, rec, root, "c30")
		cmd := exec.Command(filepath.Join(binDir, "unic"), "send", "--to", "all", "--json")
		cmd.Env = l.env
		cmd.Stdin = strings.NewReader("pipeline text")
		var stdout, stderr bytes.Buffer
		cmd.Stdout, cmd.Stderr = &stdout, &stderr
		if err := cmd.Run(); err != nil {
			t.Fatalf("submit: %v %q", err, stderr.String())
		}
		var report struct {
			Submission struct{ Phase string }
			Send       struct{ Outcome string }
		}
		if err := json.Unmarshal(stdout.Bytes(), &report); err != nil {
			t.Fatalf("stdout is not one JSON document: %q", stdout.String())
		}
		if report.Submission.Phase == "" || report.Send.Outcome == "" {
			t.Fatalf("commit and send results are not separate: %s", stdout.String())
		}
		rec.mu.Lock()
		rec.operations = append(rec.operations, map[string]any{"scenario": "C30", "commit_phase": report.Submission.Phase, "send_outcome": report.Send.Outcome})
		rec.mu.Unlock()
		l.assertNoServiceSocket()
	})

	scenario("C31", func(t *testing.T) {
		l := newLab(t, rec, root, "c31")
		p := l.unic("dev", "watch")
		inst := l.waitInstance(30*time.Second, func(v servicelifecycle.Instance) bool { return v.Leases == 1 })
		l.signal(p, syscall.SIGINT)
		p.wait(t, 15*time.Second)
		l.assertStopped(inst.PID, 15*time.Second)
		if _, stderr, code := l.run("history", "show", "missing"); code == 0 || strings.Contains(stderr, "cleanup") {
			t.Fatalf("a command error must still clean up quietly: %d %q", code, stderr)
		}
		l.assertNoServiceSocket()
	})

	scenario("C32", func(t *testing.T) {
		// A borrowed or replacement generation is never force-stopped: a stuck retiring generation
		// without a launcher handle is only reported.
		l := newLab(t, rec, root, "c32", "UNIC_E2E_RETIRE_HOLD_MS=20000")
		starter := l.unic("dev", "watch")
		inst := l.waitInstance(30*time.Second, func(v servicelifecycle.Instance) bool { return v.Leases == 1 })
		borrower := l.unic("dev", "watch")
		l.waitInstance(10*time.Second, func(v servicelifecycle.Instance) bool { return v.Leases == 2 })
		l.signal(starter, syscall.SIGKILL)
		starter.wait(t, 5*time.Second)
		l.signal(borrower, syscall.SIGINT)
		borrower.wait(t, 25*time.Second)
		if borrower.code != 5 || !strings.Contains(borrower.err.String(), "cleanup") {
			t.Fatalf("a borrower that cannot confirm shutdown must report it: %d %q", borrower.code, borrower.err.String())
		}
		if !alive(inst.PID) && unicdCount() > 1 {
			t.Fatal("unexpected process state")
		}
		l.assertStopped(inst.PID, 30*time.Second)
	})

	scenario("C33", func(t *testing.T) {
		l := newLab(t, rec, root, "c33")
		var watchers []*proc
		first := l.unic("dev", "watch")
		watchers = append(watchers, first)
		l.waitInstance(30*time.Second, func(v servicelifecycle.Instance) bool { return v.Leases == 1 })
		for range servicelifecycle.MaxLeases + 4 {
			watchers = append(watchers, l.unic("dev", "watch"))
		}
		l.waitInstance(30*time.Second, func(v servicelifecycle.Instance) bool { return v.Leases == servicelifecycle.MaxLeases })
		rejected := 0
		deadline := time.Now().Add(10 * time.Second)
		for rejected < 4 && time.Now().Before(deadline) {
			rejected = 0
			for _, w := range watchers {
				if w.exited() && w.code != 0 {
					rejected++
				}
			}
			time.Sleep(100 * time.Millisecond)
		}
		if rejected < 4 {
			t.Fatalf("expected at least 4 rejections at capacity, got %d", rejected)
		}
		for _, w := range watchers {
			if !w.exited() {
				l.signal(w, syscall.SIGINT)
			}
		}
		for _, w := range watchers {
			w.wait(t, 30*time.Second)
		}
		deadline = time.Now().Add(20 * time.Second)
		for unicdCount() != 0 && time.Now().Before(deadline) {
			time.Sleep(100 * time.Millisecond)
		}
		if unicdCount() != 0 {
			t.Fatal("capacity pressure left a service running")
		}
		if _, _, code := l.run("status"); code != 0 {
			t.Fatal("normal capacity did not return after release")
		}
	})

	scenario("C35", func(t *testing.T) {
		// Startup latency is not a failure: a slow but successful start must complete the command.
		l := newLab(t, rec, root, "c35", "UNIC_E2E_START_DELAY_MS=3000")
		began := time.Now()
		out, _, code := l.run("status")
		if code != 0 || !strings.Contains(out, "device") || time.Since(began) < 3*time.Second {
			t.Fatalf("slow start: %d after %s", code, time.Since(began))
		}
	})

	rec.mu.Lock()
	if r, ok := rec.results["C16"]; ok {
		rec.results["C17"] = map[string]any{"status": r["status"], "detail": "asserted inside the C16 scenario: the affected watch exits 4 with an interruption message"}
	}
	rec.mu.Unlock()
	writeLifecycleArtifacts(t, rec, artifacts)
}

func (l *lab) assertNoServiceSocket() {
	l.t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		if _, err := os.Stat(l.sock); err != nil {
			return
		}
		time.Sleep(50 * time.Millisecond)
	}
	l.t.Fatalf("service socket still present")
}

func dirNames(dir string) []string {
	entries, _ := os.ReadDir(dir)
	var out []string
	for _, e := range entries {
		out = append(out, e.Name())
	}
	return out
}

// writeLifecycleArtifacts writes the sanitized, replayable receipt and enforces completeness.
func writeLifecycleArtifacts(t *testing.T, rec *recorder, dir string) {
	rec.mu.Lock()
	defer rec.mu.Unlock()
	_ = os.MkdirAll(dir, 0o700)
	write := func(name string, v any) {
		b, _ := json.MarshalIndent(v, "", "  ")
		_ = os.WriteFile(filepath.Join(dir, name), append(b, '\n'), 0o600)
	}
	scenarios := map[string]any{}
	for _, id := range lifecycleScenarios {
		r, ok := rec.results[id]
		if !ok {
			t.Errorf("scenario %s did not run", id)
			continue
		}
		scenarios[id] = r
	}
	for id, reason := range notRun {
		scenarios[id] = map[string]any{"status": "NOT_RUN", "detail": reason}
	}
	var lines bytes.Buffer
	for _, e := range rec.timeline {
		b, _ := json.Marshal(e)
		lines.Write(b)
		lines.WriteByte('\n')
	}
	_ = os.WriteFile(filepath.Join(dir, "timeline.jsonl"), lines.Bytes(), 0o600)
	write("scenarios.json", scenarios)
	write("cleanup.json", rec.cleanup)
	write("operations.json", rec.operations)
	write("resource-summary.json", rec.measures)
	hashes := map[string]string{}
	for _, name := range []string{"unic", "unicd"} {
		if data, err := os.ReadFile(filepath.Join(binDir, name)); err == nil {
			sum := sha256.Sum256(data)
			hashes[name] = hex.EncodeToString(sum[:])
		}
	}
	// Commands keep names and arguments only; no environment values enter the receipt.
	write("commands.json", map[string]any{"commands": rec.commands, "environment_keys": []string{"UNIC_HOME", "UNIC_SOCK", "UNIC_RENDEZVOUS_URL", "UNIC_E2E_RETIRE_HOLD_MS", "UNIC_E2E_START_DELAY_MS"}})
	revision, _ := exec.Command("git", "rev-parse", "HEAD").Output()
	dirty, _ := exec.Command("git", "status", "--porcelain").Output()
	write("manifest.json", map[string]any{
		"source_head": strings.TrimSpace(string(revision)), "worktree_dirty": len(bytes.TrimSpace(dirty)) > 0,
		"build_tags": []string{"unic_e2e"}, "binary_sha256": hashes,
		"replay_ran": false, "note": "fixture controls only: UNIC_E2E_START_DELAY_MS, UNIC_E2E_RETIRE_HOLD_MS",
	})
	_ = os.WriteFile(filepath.Join(dir, "replay.sh"), []byte("#!/bin/sh\nset -e\ncd \"$(git rev-parse --show-toplevel)\"\nmake service-lifecycle-e2e\n"), 0o700)
	var sums bytes.Buffer
	for _, name := range dirNames(dir) {
		if name == "SHA256SUMS" {
			continue
		}
		if data, err := os.ReadFile(filepath.Join(dir, name)); err == nil {
			sum := sha256.Sum256(data)
			fmt.Fprintf(&sums, "%s  %s\n", hex.EncodeToString(sum[:]), name)
		}
	}
	_ = os.WriteFile(filepath.Join(dir, "SHA256SUMS"), sums.Bytes(), 0o600)
}

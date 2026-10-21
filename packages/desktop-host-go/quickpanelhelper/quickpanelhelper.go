// Package quickpanelhelper supervises the native quick panel helper process
// (`uniclip-quick-panel`) for desktop hosts. The helper owns the global shortcut, the
// modifier double-tap trigger and the panel window; the host only starts, restarts and
// stops it, carries out the few requests it prints on stdout, and tells it which global shortcut to hold (ADR-013
// section 10). The supervision rules
// (restart backoff, giving up, graceful stop) were ported from crates/uc-desktop/src/quick_panel_helper.rs at
// ed778b239f52c7da5c83532342e53b517e8e6bf9; this package is their only implementation now.
package quickpanelhelper

import (
	"bufio"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"sync/atomic"
	"time"
)

// ExitWhenStdinCloses makes the helper exit when the host disappears, so it never outlives it.
const ExitWhenStdinCloses = "--exit-when-stdin-closes"

// ExeStem is the helper executable name without extension.
const ExeStem = "uniclip-quick-panel"

const (
	restartBaseDelay    = time.Second
	restartMaxDelay     = 30 * time.Second
	stableAfter         = time.Minute
	maxConsecutiveFails = 5
	gracefulExit        = time.Second
	tickInterval        = 500 * time.Millisecond
	// replyTimeout bounds how long a shortcut command waits for the helper's answer.
	replyTimeout = 3 * time.Second
)

// Request is something the helper asks the host to do.
type Request string

const (
	ShowMainWindow Request = "show_main_window"
	OpenSettings   Request = "open_settings"
)

// ParseRequest decodes one line of helper output; anything that is not a known request is noise.
func ParseRequest(line string) (Request, bool) {
	var msg struct {
		Request any `json:"request"`
	}
	if json.Unmarshal([]byte(strings.TrimSpace(line)), &msg) != nil {
		return "", false
	}
	name, _ := msg.Request.(string)
	switch r := Request(name); r {
	case ShowMainWindow, OpenSettings:
		return r, true
	}
	return "", false
}

// ShortcutReply is the helper's answer to `set_shortcuts`: the bindings it holds now, and the error when it refused
// the requested ones (it then still holds the previous ones).
type ShortcutReply struct {
	Seq    uint64   `json:"seq"`
	Active []string `json:"active"`
	Error  string   `json:"error,omitempty"`
}

// parseShortcutReply decodes a `{"shortcuts":{...}}` line.
func parseShortcutReply(line string) (ShortcutReply, bool) {
	var msg struct {
		Shortcuts *ShortcutReply `json:"shortcuts"`
	}
	if json.Unmarshal([]byte(strings.TrimSpace(line)), &msg) != nil || msg.Shortcuts == nil {
		return ShortcutReply{}, false
	}
	return *msg.Shortcuts, true
}

// Child is a running helper.
type Child interface {
	HasExited() bool
	Terminate()
	// Send writes one command line to the helper's standard input.
	Send(line []byte) error
	// Replies delivers the helper's shortcut replies; it is never closed.
	Replies() <-chan ShortcutReply
}

// Launcher starts one helper process.
type Launcher interface {
	Launch() (Child, error)
}

// ResolveExePath finds the helper next to the host executable.
func ResolveExePath() (string, bool) {
	name := ExeStem
	if runtime.GOOS == "windows" {
		name += ".exe"
	}
	exe, err := os.Executable()
	if err != nil {
		return "", false
	}
	path := filepath.Join(filepath.Dir(exe), name)
	info, err := os.Stat(path)
	return path, err == nil && info.Mode().IsRegular()
}

// ProcessLauncher runs the helper as a child process.
type ProcessLauncher struct {
	Executable string
	Args       []string
	OnRequest  func(Request)
}

// ForHelper builds the launcher used in production.
func ForHelper(executable string, onRequest func(Request)) *ProcessLauncher {
	return &ProcessLauncher{Executable: executable, Args: []string{ExitWhenStdinCloses}, OnRequest: onRequest}
}

type processChild struct {
	cmd     *exec.Cmd
	stdin   io.WriteCloser
	done    chan struct{}
	replies chan ShortcutReply
	writeMu sync.Mutex
}

func (c *processChild) Send(line []byte) error {
	c.writeMu.Lock()
	defer c.writeMu.Unlock()
	_, err := c.stdin.Write(append(line, '\n'))
	return err
}

func (c *processChild) Replies() <-chan ShortcutReply { return c.replies }

func (c *processChild) HasExited() bool {
	select {
	case <-c.done:
		return true
	default:
		return false
	}
}

// Terminate closes the helper's stdin (its cue to exit), waits briefly, then kills it.
func (c *processChild) Terminate() {
	c.writeMu.Lock()
	_ = c.stdin.Close()
	c.writeMu.Unlock()
	select {
	case <-c.done:
	case <-time.After(gracefulExit):
		_ = c.cmd.Process.Kill()
		<-c.done
	}
}

// Launch implements Launcher.
func (l *ProcessLauncher) Launch() (Child, error) {
	cmd := exec.Command(l.Executable, l.Args...)
	configureProcess(cmd)
	stdin, err := cmd.StdinPipe()
	if err != nil {
		return nil, err
	}
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		return nil, err
	}
	stderr, err := cmd.StderrPipe()
	if err != nil {
		return nil, err
	}
	if err := cmd.Start(); err != nil {
		return nil, err
	}
	child := &processChild{cmd: cmd, stdin: stdin, done: make(chan struct{}), replies: make(chan ShortcutReply, 8)}
	var readers sync.WaitGroup
	readers.Add(2)
	go func() {
		defer readers.Done()
		scan := bufio.NewScanner(stderr)
		for scan.Scan() {
			log.Printf("quick panel helper: %s", scan.Text())
		}
	}()
	go func() {
		defer readers.Done()
		scan := bufio.NewScanner(stdout)
		for scan.Scan() {
			if reply, ok := parseShortcutReply(scan.Text()); ok {
				select {
				case child.replies <- reply:
				default: // nobody waits: a late answer to a command that already timed out
				}
			} else if request, ok := ParseRequest(scan.Text()); ok && l.OnRequest != nil {
				l.OnRequest(request)
			}
		}
	}()
	go func() {
		readers.Wait() // the pipes must be drained before Wait closes them
		_ = cmd.Wait()
		close(child.done)
	}()
	return child, nil
}

// Supervisor keeps the helper running while enabled. The zero value is not usable; call Start.
type Supervisor struct {
	mu        sync.Mutex
	launcher  Launcher
	enabled   bool
	running   *running
	failures  int
	retryAt   time.Time
	gaveUp    bool
	stop      chan struct{}
	stopped   chan struct{}
	closeOnce sync.Once

	onLaunch  func()     // called on its own goroutine after every launch; set before the first SetEnabled
	commandMu sync.Mutex // one shortcut command at a time
	seq       atomic.Uint64
}

// ErrNotRunning: no helper process runs, so it cannot take a command (and holds nothing).
var ErrNotRunning = errors.New("the quick panel helper is not running")

// OnLaunch registers fn to run after every launch of the helper (first start, enable, restart after a crash). A new
// helper holds no global shortcut until it is told one. Call it before the first SetEnabled.
func (s *Supervisor) OnLaunch(fn func()) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.onLaunch = fn
}

// ErrNoAnswer: the command was sent, or its sending failed half-way, and no matching answer came. The helper may hold
// the old bindings, the new ones or none; the next command's answer tells.
var ErrNoAnswer = errors.New("the quick panel helper did not answer")

// SetShortcuts tells the running helper which global shortcuts to hold and returns the bindings it reports holding
// afterwards. The helper replaces its bindings transactionally, so after a refusal it reports the previous ones. An
// error wrapping ErrNoAnswer means the held bindings are unknown. An answer with another seq is a late answer to an
// earlier command that timed out; it is dropped.
func (s *Supervisor) SetShortcuts(bindings []string) ([]string, error) {
	s.commandMu.Lock()
	defer s.commandMu.Unlock()
	child := s.currentChild()
	if child == nil {
		return nil, ErrNotRunning
	}
	seq := s.seq.Add(1)
	if bindings == nil {
		bindings = []string{}
	}
	line, err := json.Marshal(map[string]any{"command": "set_shortcuts", "seq": seq, "shortcuts": bindings})
	if err != nil {
		return nil, err
	}
	if err := child.Send(line); err != nil {
		return nil, fmt.Errorf("%w: %v", ErrNoAnswer, err)
	}
	timeout := time.After(replyTimeout)
	for {
		select {
		case reply := <-child.Replies():
			if reply.Seq != seq {
				log.Printf("quick panel helper: dropped a late shortcut answer (seq %d, waiting for %d)", reply.Seq, seq)
				continue
			}
			if reply.Error != "" {
				return reply.Active, errors.New(reply.Error)
			}
			return reply.Active, nil
		case <-timeout:
			return nil, ErrNoAnswer
		}
	}
}

// ReloadSettings asks the running helper to read its settings again (double tap, theme, language). Without a running
// helper there is nothing to do: the next launch reads them anyway.
func (s *Supervisor) ReloadSettings() {
	if child := s.currentChild(); child != nil {
		if err := child.Send([]byte(`{"command":"reload_settings"}`)); err != nil {
			log.Printf("quick panel helper: could not ask it to reload its settings: %v", err)
		}
	}
}

func (s *Supervisor) currentChild() Child {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.running == nil {
		return nil
	}
	return s.running.child
}

type running struct {
	child     Child
	startedAt time.Time
}

// Start creates a supervisor and its restart loop. The helper is not launched until SetEnabled(true).
func Start(launcher Launcher) *Supervisor {
	s := &Supervisor{launcher: launcher, stop: make(chan struct{}), stopped: make(chan struct{})}
	go func() {
		defer close(s.stopped)
		ticker := time.NewTicker(tickInterval)
		defer ticker.Stop()
		for {
			select {
			case <-s.stop:
				return
			case now := <-ticker.C:
				s.tick(now)
			}
		}
	}()
	return s
}

// SetEnabled starts or stops the helper to match the persisted setting.
func (s *Supervisor) SetEnabled(enabled bool) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.enabled = enabled
	if enabled {
		s.resetFailures()
		s.startLocked(time.Now())
	} else {
		s.stopLocked()
	}
}

// Restart relaunches a running helper so it re-reads settings it only applies at startup.
func (s *Supervisor) Restart() {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.stopLocked()
	if s.enabled {
		s.resetFailures()
		s.startLocked(time.Now())
	}
}

// Running reports whether a helper process is currently supervised.
func (s *Supervisor) Running() bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.running != nil
}

// GaveUp reports whether the helper kept failing and will not be restarted again.
func (s *Supervisor) GaveUp() bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.gaveUp
}

// Shutdown stops the helper for good and ends the restart loop.
func (s *Supervisor) Shutdown() {
	s.mu.Lock()
	s.enabled = false
	s.stopLocked()
	s.mu.Unlock()
	s.closeOnce.Do(func() { close(s.stop) })
	<-s.stopped
}

func (s *Supervisor) tick(now time.Time) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.running != nil {
		if !s.running.child.HasExited() {
			return
		}
		ranFor := now.Sub(s.running.startedAt)
		s.running = nil
		if s.enabled {
			s.recordFailure(ranFor, now)
		}
	}
	if s.enabled && !s.gaveUp && s.running == nil && !s.retryAt.IsZero() && !now.Before(s.retryAt) {
		s.startLocked(now)
	}
}

func (s *Supervisor) startLocked(now time.Time) {
	if s.running != nil {
		return
	}
	s.retryAt = time.Time{}
	child, err := s.launcher.Launch()
	if err != nil {
		log.Printf("quick panel helper: failed to start: %v", err)
		s.recordFailure(0, now)
		return
	}
	log.Printf("quick panel helper: started")
	s.running = &running{child: child, startedAt: now}
	if s.onLaunch != nil {
		go s.onLaunch()
	}
}

func (s *Supervisor) stopLocked() {
	s.retryAt = time.Time{}
	if s.running != nil {
		s.running.child.Terminate()
		s.running = nil
		log.Printf("quick panel helper: stopped")
	}
}

func (s *Supervisor) resetFailures() {
	s.failures, s.gaveUp, s.retryAt = 0, false, time.Time{}
}

func (s *Supervisor) recordFailure(ranFor time.Duration, now time.Time) {
	if ranFor >= stableAfter {
		s.failures = 0
	}
	s.failures++
	if s.failures > maxConsecutiveFails {
		s.gaveUp, s.retryAt = true, time.Time{}
		log.Printf("quick panel helper: keeps failing; not restarting it again")
		return
	}
	delay := min(restartBaseDelay<<min(s.failures-1, 16), restartMaxDelay)
	s.retryAt = now.Add(delay)
	log.Printf("quick panel helper: exited; restarting in %s", delay)
}

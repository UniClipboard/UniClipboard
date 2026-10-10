// Package quickpanelhelper supervises the native quick panel helper process
// (`uniclip-quick-panel`) for desktop hosts. The helper owns the global shortcut, the
// modifier double-tap trigger and the panel window; the host only starts, restarts and
// stops it and carries out the few requests it prints on stdout. The supervision rules
// (restart backoff, giving up, graceful stop) were ported from crates/uc-desktop/src/quick_panel_helper.rs at
// ed778b239f52c7da5c83532342e53b517e8e6bf9; this package is their only implementation now.
package quickpanelhelper

import (
	"bufio"
	"encoding/json"
	"io"
	"log"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
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

// Child is a running helper.
type Child interface {
	HasExited() bool
	Terminate()
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
	cmd   *exec.Cmd
	stdin io.WriteCloser
	done  chan struct{}
}

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
	_ = c.stdin.Close()
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
	child := &processChild{cmd: cmd, stdin: stdin, done: make(chan struct{})}
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
			if request, ok := ParseRequest(scan.Text()); ok && l.OnRequest != nil {
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

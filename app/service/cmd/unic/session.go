package main

import (
	"context"
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"sync"
	"syscall"
	"time"

	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/internal/servicelaunch"
	"github.com/uniclipboard/unic/internal/servicelifecycle"
)

// Frozen lifecycle budgets. Startup time is separate from business time:
// requests carry their own deadline that starts after acquisition.
const (
	probeTimeout   = time.Second
	acquireBudget  = 45 * time.Second // includes waiting for a retiring generation
	startBudget    = 30 * time.Second // readiness of one launched unicd
	gracefulBudget = 12 * time.Second // last-user shutdown
	forcedBudget   = 5 * time.Second  // after a forced kill of our own child
	requestTimeout = 60 * time.Second // one non-streaming request
)

// acquireError marks failures of service acquisition so they print without HTTP framing.
type acquireError struct{ err error }

func (e *acquireError) Error() string { return e.err.Error() }
func (e *acquireError) Unwrap() error { return e.err }

// session is one CLI invocation's usage of a service generation.
// It acquires lazily: syntax errors are reported before any service starts.
type session struct {
	dir, socket string
	client      *api.Client // for commands; acquires the service on first request
	raw         *api.Client // for lifecycle control; never acquires

	mu       sync.Mutex
	tried    bool
	err      error
	lease    *api.Lease
	instance servicelifecycle.Instance
	child    *child // set only when this process launched the generation
}

// child is a unicd launched by this invocation.
type child struct {
	cmd   *exec.Cmd
	owner *os.File // initial ownership; closing it releases the launcher's claim
	done  chan struct{}
	err   error
}

func newSession(dir, socket string) *session {
	s := &session{dir: dir, socket: socket, client: api.NewClient(socket), raw: api.NewClient(socket)}
	s.client.WrapTransport(func(base http.RoundTripper) http.RoundTripper {
		return acquiring{base: base, ensure: s.ensure}
	})
	return s
}

// denied returns a client for local commands. Any service request is a programming error
// because the command's declaration says it needs no service.
func deniedClient(socket string) *api.Client {
	c := api.NewClient(socket)
	c.WrapTransport(func(http.RoundTripper) http.RoundTripper { return denied{} })
	return c
}

type denied struct{}

func (denied) RoundTrip(*http.Request) (*http.Response, error) {
	return nil, &acquireError{errors.New("internal error: a command declared local requested the service")}
}

type acquiring struct {
	base   http.RoundTripper
	ensure func(context.Context) error
}

func (t acquiring) RoundTrip(req *http.Request) (*http.Response, error) {
	if err := t.ensure(req.Context()); err != nil {
		return nil, err
	}
	if req.URL.Path == "/v1/events" || req.URL.Path == "/v1/service/leases" || req.URL.Path == "/v1/receive" {
		return t.base.RoundTrip(req)
	}
	ctx, cancel := context.WithTimeout(req.Context(), requestTimeout)
	resp, err := t.base.RoundTrip(req.WithContext(ctx))
	if err != nil {
		cancel()
		return nil, err
	}
	resp.Body = cancelOnClose{resp.Body, cancel}
	return resp, nil
}

type cancelOnClose struct {
	io.ReadCloser
	cancel context.CancelFunc
}

func (b cancelOnClose) Close() error {
	defer b.cancel()
	return b.ReadCloser.Close()
}

func (s *session) ensure(ctx context.Context) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.tried {
		return s.err
	}
	s.tried = true
	if err := s.acquire(ctx); err != nil {
		s.err = &acquireError{err}
	}
	return s.err
}

// temporary reports whether this invocation uses a CLI-temporary generation.
func (s *session) temporary() bool {
	if s == nil {
		return false
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.lease != nil && s.instance.Mode == servicelifecycle.ModeCLITemporary
}

// started returns when the used generation began.
func (s *session) startedAt() (time.Time, bool) {
	if s == nil {
		return time.Time{}, false
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.lease == nil {
		return time.Time{}, false
	}
	return time.UnixMilli(s.instance.StartedAtMs), true
}

type retryAfter struct{ id string }

func (retryAfter) Error() string { return "service generation is retiring" }

// closedByPeer reports a connection that the service closed without an answer.
func closedByPeer(err error) bool {
	return errors.Is(err, io.EOF) || errors.Is(err, io.ErrUnexpectedEOF) ||
		errors.Is(err, syscall.ECONNRESET) || errors.Is(err, syscall.EPIPE)
}

func (s *session) acquire(ctx context.Context) error {
	ctx, cancel := context.WithTimeout(ctx, acquireBudget)
	defer cancel()
	for {
		err := s.attempt(ctx)
		var retry retryAfter
		if !errors.As(err, &retry) {
			return err
		}
		// Retry only the control plane, before any business request was admitted.
		if retry.id == "" {
			// The previous generation closed its connection and its identity is unknown. Back off briefly.
			select {
			case <-ctx.Done():
				return fmt.Errorf("wait for previous unicd shutdown: %w", ctx.Err())
			case <-time.After(50 * time.Millisecond):
			}
			continue
		}
		if err := s.waitGone(ctx, retry.id, nil); err != nil {
			return fmt.Errorf("wait for previous unicd shutdown: %w", err)
		}
	}
}

func (s *session) attempt(ctx context.Context) error {
	unlock, err := servicelaunch.Lock(s.dir)
	if err != nil {
		return err
	}
	defer unlock()
	probe, cancel := context.WithTimeout(ctx, probeTimeout)
	inst, err := s.raw.Service(probe)
	cancel()
	switch {
	case err == nil:
		if inst.Directory != s.dir {
			return errors.New("socket belongs to a different state directory")
		}
		if inst.Retiring {
			return retryAfter{inst.ID}
		}
		return s.take(ctx, inst, nil)
	case errors.Is(err, api.ErrNoDaemon):
		return s.start(ctx)
	case closedByPeer(err):
		// A generation that is exiting closes connections it already accepted.
		return retryAfter{}
	}
	// An unrecognized listener is never killed or unlinked by the CLI.
	return fmt.Errorf("inspect existing unicd: %w", err)
}

func (s *session) take(ctx context.Context, inst servicelifecycle.Instance, ch *child) error {
	l, err := s.raw.AcquireLease(ctx, inst.ID)
	switch {
	case errors.Is(err, api.ErrRetiring), errors.Is(err, api.ErrGenerationChanged):
		return retryAfter{inst.ID}
	case ch == nil && (closedByPeer(err) || errors.Is(err, api.ErrNoDaemon)):
		// The generation stopped between the probe and lease admission. No lease exists yet.
		return retryAfter{inst.ID}
	case err != nil:
		return err
	}
	s.lease, s.instance, s.child = l, inst, ch
	return nil
}

func (s *session) start(ctx context.Context) error {
	exe, err := servicelaunch.Sibling()
	if err != nil {
		return err
	}
	if err := servicelaunch.CheckExecutable(exe); err != nil {
		return err
	}
	if err := os.MkdirAll(s.dir, 0o700); err != nil {
		return err
	}
	startCtx, cancel := context.WithTimeout(ctx, startBudget)
	defer cancel()
	for {
		ch, inst, err := s.spawn(startCtx, exe)
		var exit *exec.ExitError
		if err == nil {
			if err := s.take(ctx, inst, ch); err != nil {
				s.stopChild(ch)
				return err
			}
			// The lease now keeps the generation alive; release the launcher's claim.
			_ = ch.owner.Close()
			return nil
		}
		if !errors.As(err, &exit) || exit.ExitCode() != servicelifecycle.ExitDirectoryInUse {
			return err
		}
		// An earlier owner is still releasing the directory. Retry only this typed code.
		timer := time.NewTimer(100 * time.Millisecond)
		select {
		case <-startCtx.Done():
			timer.Stop()
			return fmt.Errorf("wait for state directory: %w", startCtx.Err())
		case <-timer.C:
		}
	}
}

func (s *session) spawn(ctx context.Context, exe string) (*child, servicelifecycle.Instance, error) {
	logFile, err := os.OpenFile(filepath.Join(s.dir, "cli-daemon.log"), os.O_CREATE|os.O_WRONLY|os.O_TRUNC, 0o600)
	if err != nil {
		return nil, servicelifecycle.Instance{}, err
	}
	defer func() { _ = logFile.Close() }()
	read, write, err := os.Pipe()
	if err != nil {
		return nil, servicelifecycle.Instance{}, err
	}
	defer func() { _ = read.Close() }()
	cmd := exec.Command(exe, "--cli-temporary")
	servicelaunch.ConfigureProcess(cmd)
	// Preserve the selected profile and socket. The command's own stdin stays untouched.
	cmd.Env = append(os.Environ(), "UNIC_HOME="+s.dir, "UNIC_SOCK="+s.socket)
	cmd.Stdin, cmd.Stdout, cmd.Stderr = read, logFile, logFile
	if err := cmd.Start(); err != nil {
		_ = write.Close()
		return nil, servicelifecycle.Instance{}, err
	}
	ch := &child{cmd: cmd, owner: write, done: make(chan struct{})}
	go func() { ch.err = cmd.Wait(); close(ch.done) }()
	var inst servicelifecycle.Instance
	err = servicelaunch.Wait(ctx, func() (bool, error) {
		select {
		case <-ch.done:
			if ch.err != nil {
				return false, fmt.Errorf("unicd exited before readiness: %w; see cli-daemon.log", ch.err)
			}
			return false, errors.New("unicd exited before readiness; see cli-daemon.log")
		default:
		}
		probe, cancel := context.WithTimeout(ctx, probeTimeout)
		defer cancel()
		v, err := s.raw.Service(probe)
		if err != nil {
			return false, nil
		}
		inst = v
		return v.PID == cmd.Process.Pid && v.Directory == s.dir && v.Mode == servicelifecycle.ModeCLITemporary, nil
	})
	if err != nil {
		s.stopChild(ch)
		return nil, inst, err
	}
	return ch, inst, nil
}

// stopChild ends a generation this invocation created and never leased.
func (s *session) stopChild(ch *child) {
	_ = ch.owner.Close()
	select {
	case <-ch.done:
		return
	case <-time.After(gracefulBudget):
	}
	_ = ch.cmd.Process.Kill()
	select {
	case <-ch.done:
	case <-time.After(forcedBudget):
	}
}

// waitGone returns when generation id no longer answers on the socket.
func (s *session) waitGone(ctx context.Context, id string, done <-chan struct{}) error {
	return servicelaunch.Wait(ctx, func() (bool, error) {
		if done != nil {
			select {
			case <-done:
				return true, nil
			default:
			}
		}
		probe, cancel := context.WithTimeout(ctx, probeTimeout)
		defer cancel()
		v, err := s.raw.Service(probe)
		if errors.Is(err, api.ErrNoDaemon) || (err == nil && v.ID != id) {
			return true, nil
		}
		return false, nil
	})
}

// Release ends this invocation's usage with its own bounded context.
// When it retires a temporary generation it waits for that generation to stop.
func (s *session) Release() error {
	if s == nil {
		return nil
	}
	s.mu.Lock()
	lease, inst, ch := s.lease, s.instance, s.child
	s.lease = nil
	s.mu.Unlock()
	if lease == nil {
		return nil
	}
	ctx, cancel := context.WithTimeout(context.Background(), probeTimeout*5)
	last, err := lease.Release(ctx)
	cancel()
	if err != nil {
		// A replaced or dead service has nothing left to release.
		if errors.Is(err, api.ErrNoDaemon) || errors.Is(err, api.ErrGenerationChanged) {
			return nil
		}
		return fmt.Errorf("release unicd usage: %w", err)
	}
	if !last {
		return nil
	}
	var done <-chan struct{}
	if ch != nil {
		done = ch.done
	}
	wait, cancelWait := context.WithTimeout(context.Background(), gracefulBudget)
	defer cancelWait()
	if err := s.waitGone(wait, inst.ID, done); err == nil {
		return nil
	}
	// Force only the generation this invocation created, now retiring with no users.
	if ch == nil {
		return errors.New("temporary unicd did not stop in time")
	}
	_ = ch.cmd.Process.Kill()
	select {
	case <-ch.done:
		return errors.New("temporary unicd required forced shutdown")
	case <-time.After(forcedBudget):
		return errors.New("temporary unicd did not terminate")
	}
}

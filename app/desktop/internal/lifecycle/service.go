// Package lifecycle owns profile-scoped desktop service startup and shutdown.
// It contains no graphical dependencies so real-process acceptance needs no display.
package lifecycle

import (
	"context"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"sync"
	"time"

	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/internal/servicelaunch"
	"github.com/uniclipboard/unic/internal/servicelifecycle"
)

// Process is a service started and owned by a desktop tray session.
// Closing its parent pipe also works if the desktop crashes or is force-killed.
type Process struct {
	cmd   *exec.Cmd
	owner *os.File
	done  chan struct{}
	err   error // published by closing done

	mu               sync.Mutex
	instance         servicelifecycle.Instance
	socket           string
	releaseAttempted bool
	detached         bool
}

// Sibling resolves the daemon from the same installation as the desktop.
func Sibling() (string, error) { return servicelaunch.Sibling() }

// AcquireService borrows a standalone service or starts a tray-owned service.
// A nil process means that the service remains independently owned. The caller
// must never stop that generation when the tray or a UI process exits.
func AcquireService(ctx context.Context, dir, socket, executable string) (*Process, error) {
	if err := servicelaunch.CheckExecutable(executable); err != nil {
		return nil, err
	}
	startupCtx, cancel := context.WithTimeout(ctx, 30*time.Second)
	defer cancel()
	unlock, err := servicelaunch.LockContext(startupCtx, dir)
	if err != nil {
		return nil, err
	}
	defer unlock()
	c := api.NewClient(socket)
	for {
		probe, stopProbe := context.WithTimeout(startupCtx, time.Second)
		existing, err := c.Service(probe)
		stopProbe()
		if err == nil {
			if existing.Directory != dir {
				return nil, errors.New("socket belongs to a different state directory")
			}
			switch existing.Mode {
			case servicelifecycle.ModeStandalone:
				if !existing.Retiring {
					return nil, nil
				}
			case servicelifecycle.ModeDesktop:
				if !existing.Retiring {
					return nil, errors.New("a desktop already owns this service; retry after it exits")
				}
			case servicelifecycle.ModeCLITemporary:
				return nil, errors.New("a unic command is using a temporary service; retry after it finishes")
			default:
				return nil, errors.New("unrecognized service ownership mode")
			}
		} else if !errors.Is(err, api.ErrNoDaemon) {
			return nil, fmt.Errorf("inspect existing unicd: %w", err)
		} else {
			process, startErr := startProcess(startupCtx, dir, socket, executable, true)
			var exit *exec.ExitError
			if !errors.As(startErr, &exit) || exit.ExitCode() != servicelifecycle.ExitDirectoryInUse {
				return process, startErr
			}
		}
		// A retiring generation may have closed its listener before Engine
		// releases the profile. Never remove its socket or send it a stop request.
		timer := time.NewTimer(100 * time.Millisecond)
		select {
		case <-startupCtx.Done():
			timer.Stop()
			return nil, fmt.Errorf("wait for service retirement: %w", startupCtx.Err())
		case <-timer.C:
		}
	}
}

// Start serializes launcher transitions for one state directory.
// Normal mode replaces an existing service. Lightweight mode reuses a standalone service.
// A nil process means that only the standalone service remains.
func Start(ctx context.Context, dir, socket, executable string, normal bool) (*Process, error) {
	if err := servicelaunch.CheckExecutable(executable); err != nil {
		return nil, err
	}
	unlock, err := servicelaunch.Lock(dir)
	if err != nil {
		return nil, err
	}
	defer unlock()
	c := api.NewClient(socket)
	probe, cancel := context.WithTimeout(ctx, time.Second)
	existing, err := c.Service(probe)
	cancel()
	if err == nil {
		if existing.Directory != dir {
			return nil, errors.New("socket belongs to a different state directory")
		}
		if !normal {
			switch existing.Mode {
			case servicelifecycle.ModeDesktop:
				return nil, errors.New("quit the normal desktop before starting lightweight mode")
			case servicelifecycle.ModeCLITemporary:
				return nil, errors.New("a unic command is using a temporary service; retry after it finishes")
			}
			return nil, nil
		}
		stopCtx, stopCancel := context.WithTimeout(ctx, 15*time.Second)
		defer stopCancel()
		if err := c.StopService(stopCtx, existing.ID); err != nil {
			return nil, err
		}
		// The daemon removes its socket after releasing the Engine lock.
		if err := servicelaunch.Wait(stopCtx, func() (bool, error) {
			_, err := os.Stat(socket)
			return errors.Is(err, os.ErrNotExist), nil
		}); err != nil {
			return nil, fmt.Errorf("wait for previous unicd shutdown: %w", err)
		}
	} else if !errors.Is(err, api.ErrNoDaemon) {
		// An unrecognized listener is never killed or unlinked by the launcher.
		return nil, fmt.Errorf("inspect existing unicd: %w", err)
	}

	// A previous owner may have died after its socket closed but before Engine unlocked.
	startupCtx, startupCancel := context.WithTimeout(ctx, 30*time.Second)
	defer startupCancel()
	for {
		process, err := startProcess(startupCtx, dir, socket, executable, normal)
		var exit *exec.ExitError
		if !errors.As(err, &exit) || exit.ExitCode() != servicelifecycle.ExitDirectoryInUse {
			return process, err
		}
		timer := time.NewTimer(100 * time.Millisecond)
		select {
		case <-startupCtx.Done():
			timer.Stop()
			return nil, fmt.Errorf("wait for state directory: %w", startupCtx.Err())
		case <-timer.C:
		}
	}
}

func startProcess(ctx context.Context, dir, socket, executable string, normal bool) (*Process, error) {
	c := api.NewClient(socket)
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	cmd := exec.Command(executable)
	configureProcess(cmd)
	// Preserve the selected profile and socket instead of inheriting ambiguous defaults.
	cmd.Env = append(os.Environ(), "UNIC_HOME="+dir, "UNIC_SOCK="+socket)
	log, err := os.OpenFile(filepath.Join(dir, "desktop-daemon.log"), os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0600)
	if err != nil {
		return nil, err
	}
	defer func() { _ = log.Close() }()
	cmd.Stdout, cmd.Stderr = log, log
	p := &Process{cmd: cmd, done: make(chan struct{}), socket: socket}
	var read *os.File
	if normal {
		read, p.owner, err = os.Pipe()
		if err != nil {
			return nil, err
		}
		defer func() { _ = read.Close() }()
		cmd.Args = append(cmd.Args, "--desktop-owned")
		cmd.Stdin = read
	}
	if err := cmd.Start(); err != nil {
		if p.owner != nil {
			_ = p.owner.Close()
		}
		return nil, err
	}
	go func() { p.err = cmd.Wait(); close(p.done) }()
	mode := servicelifecycle.ModeStandalone
	if normal {
		mode = servicelifecycle.ModeDesktop
	}
	readyCtx, readyCancel := context.WithTimeout(ctx, 30*time.Second)
	defer readyCancel()
	err = servicelaunch.Wait(readyCtx, func() (bool, error) {
		select {
		case <-p.done:
			if p.err != nil {
				return false, fmt.Errorf("unicd exited before readiness: %w; see desktop-daemon.log", p.err)
			}
			return false, errors.New("unicd exited before readiness; see desktop-daemon.log")
		default:
		}
		probeCtx, probeCancel := context.WithTimeout(readyCtx, time.Second)
		defer probeCancel()
		instance, err := c.Service(probeCtx)
		ready := err == nil && instance.PID == cmd.Process.Pid && instance.Directory == dir && instance.Mode == mode
		if ready {
			p.instance = instance
		}
		return ready, nil
	})
	if err != nil {
		_ = p.Stop()
		return nil, err
	}
	if !normal {
		return nil, nil
	}
	return p, nil
}

// Done closes when the child terminates, including after an ownership release.
func (p *Process) Done() <-chan struct{} { return p.done }

// Detach releases this exact owned generation without restarting Engine. The
// exclusive pipe carries one fixed marker and EOF. Repeated calls only query
// the original generation. An uncertain release never authorizes Stop or kill.
func (p *Process) Detach(ctx context.Context) error {
	if p == nil {
		return nil // borrowed standalone services have no ownership to release
	}
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.detached {
		return nil
	}
	var writeErr error
	if !p.releaseAttempted {
		if err := ctx.Err(); err != nil {
			return err
		}
		if p.owner == nil {
			return errors.New("service has no exclusive desktop owner pipe")
		}
		p.releaseAttempted = true
		_, writeErr = p.owner.Write([]byte(servicelifecycle.OwnerReleaseMarker))
		writeErr = errors.Join(writeErr, p.owner.Close())
		p.owner = nil
	}
	queryCtx, cancel := context.WithTimeout(ctx, 30*time.Second)
	defer cancel()
	err := servicelaunch.Wait(queryCtx, func() (bool, error) {
		select {
		case <-p.done:
			return false, errors.New("owned service exited before release confirmation")
		default:
		}
		probe, stopProbe := context.WithTimeout(queryCtx, time.Second)
		defer stopProbe()
		current, err := api.NewClient(p.socket).Service(probe)
		if err != nil {
			return false, nil // an uncertain result permits only another query
		}
		if current.ID != p.instance.ID || current.PID != p.instance.PID || current.Directory != p.instance.Directory {
			return false, errors.New("service generation changed during ownership release")
		}
		if current.Retiring {
			return false, errors.New("owned service is retiring")
		}
		if current.Mode == servicelifecycle.ModeStandalone {
			p.detached = true
			return true, nil
		}
		if current.Mode != servicelifecycle.ModeDesktop {
			return false, errors.New("service ownership mode changed during release")
		}
		return false, nil
	})
	if err == nil {
		return nil // the exact generation's authoritative receipt resolves an uncertain write
	}
	return errors.Join(writeErr, err)
}

// Stop closes the owner pipe, waits for graceful shutdown, then bounds forced cleanup.
func (p *Process) Stop() error {
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.detached {
		return nil
	}
	if p.releaseAttempted {
		return errors.New("service ownership release is unconfirmed; query the original generation")
	}
	if p.owner != nil {
		_ = p.owner.Close()
	} else {
		_ = p.cmd.Process.Kill()
	}
	select {
	case <-p.done:
		return p.err
	case <-time.After(12 * time.Second):
		_ = p.cmd.Process.Kill()
	}
	select {
	case <-p.done:
		return errors.New("unicd required forced shutdown")
	case <-time.After(5 * time.Second):
		return errors.New("unicd did not terminate")
	}
}

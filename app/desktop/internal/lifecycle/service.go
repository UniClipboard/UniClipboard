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
	"runtime"
	"time"

	"github.com/rogpeppe/go-internal/lockedfile"

	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/internal/servicelifecycle"
)

// Process is a service started for a normal desktop session.
// Closing its parent pipe also works if the desktop crashes or is force-killed.
type Process struct {
	cmd   *exec.Cmd
	owner *os.File
	done  chan struct{}
	err   error // published by closing done
}

// Sibling resolves the daemon from the same installation as the desktop.
func Sibling() (string, error) {
	executable, err := os.Executable()
	if err != nil {
		return "", err
	}
	executable, err = filepath.EvalSymlinks(executable)
	if err != nil {
		return "", err
	}
	name := "unicd"
	if runtime.GOOS == "windows" {
		name += ".exe"
	}
	return filepath.Join(filepath.Dir(executable), name), nil
}

// Start serializes launcher transitions for one state directory.
// Normal mode replaces an existing service. Lightweight mode reuses a standalone service.
// A nil process means that only the standalone service remains.
func Start(ctx context.Context, dir, socket, executable string, normal bool) (*Process, error) {
	info, err := os.Stat(executable)
	if err != nil {
		return nil, fmt.Errorf("locate unicd beside unic-desktop: %w", err)
	}
	if info.IsDir() || (runtime.GOOS != "windows" && info.Mode()&0111 == 0) {
		return nil, errors.New("unicd is not an executable file")
	}
	unlock, err := lockedfile.MutexAt(filepath.Join(dir, "desktop-startup.lock")).Lock()
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
			if existing.DesktopOwned {
				return nil, errors.New("quit the normal desktop before starting lightweight mode")
			}
			return nil, nil
		}
		stopCtx, stopCancel := context.WithTimeout(ctx, 15*time.Second)
		defer stopCancel()
		if err := c.StopService(stopCtx, existing.ID); err != nil {
			return nil, err
		}
		// The daemon removes its socket after releasing the Engine lock.
		if err := wait(stopCtx, func() (bool, error) {
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
	p := &Process{cmd: cmd, done: make(chan struct{})}
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
	readyCtx, readyCancel := context.WithTimeout(ctx, 30*time.Second)
	defer readyCancel()
	err = wait(readyCtx, func() (bool, error) {
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
		return err == nil && instance.PID == cmd.Process.Pid && instance.Directory == dir && instance.DesktopOwned == normal, nil
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

// Done closes when the child terminates. A normal GUI must not outlive its service.
func (p *Process) Done() <-chan struct{} { return p.done }

// Stop closes the owner pipe, waits for graceful shutdown, then bounds forced cleanup.
func (p *Process) Stop() error {
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

func wait(ctx context.Context, check func() (bool, error)) error {
	ticker := time.NewTicker(50 * time.Millisecond)
	defer ticker.Stop()
	for {
		if done, err := check(); done || err != nil {
			return err
		}
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-ticker.C:
		}
	}
}

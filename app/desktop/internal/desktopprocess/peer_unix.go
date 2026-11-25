//go:build !windows

package desktopprocess

import (
	"context"
	"errors"
	"net"
	"os"
	"path/filepath"
	"strconv"
	"syscall"
	"time"

	"github.com/tailscale/peercred"
)

func peerExecutable(conn net.Conn, _ bool) (string, int, error) {
	cred, err := peercred.Get(conn)
	if err != nil {
		return "", 0, err
	}
	uid, ok := cred.UserID()
	if !ok || uid != strconv.Itoa(os.Geteuid()) {
		return "", 0, errors.New("desktop IPC peer user mismatch")
	}
	pid, ok := cred.PID()
	if !ok {
		return "", 0, errors.New("desktop IPC peer PID unavailable")
	}
	path, err := processExecutable(pid)
	return path, pid, err
}

func retireStalePeer(ctx context.Context, conn net.Conn, root string, expected Info) (bool, error) {
	rawPath, pid, err := peerExecutable(conn, false)
	if err != nil {
		return false, err
	}
	if pid <= 0 || pid == os.Getpid() {
		return false, errors.New("cannot retire current process")
	}
	if resolved, evalErr := filepath.EvalSymlinks(rawPath); evalErr == nil && samePath(resolved, expected.BinaryPath) {
		if hash, herr := fileHash(resolved); herr == nil && hash == expected.BinarySHA256 {
			return false, nil
		}
	} else if !isReplacedExecutable(rawPath, expected.BinaryPath) {
		return false, errors.New("desktop IPC peer executable mismatch")
	}
	if err := syscall.Kill(pid, syscall.SIGTERM); err != nil {
		if errors.Is(err, syscall.ESRCH) {
			return true, nil
		}
		return false, err
	}
	if waitProcessExit(ctx, pid, root, expected.Role, 2*time.Second) {
		return true, nil
	}
	if current, err := processExecutable(pid); err == nil && current == rawPath {
		if err := syscall.Kill(pid, syscall.SIGKILL); err != nil && !errors.Is(err, syscall.ESRCH) {
			return false, err
		}
	}
	if waitProcessExit(ctx, pid, root, expected.Role, 3*time.Second) {
		return true, nil
	}
	return false, errors.New("timed out waiting for stale desktop role to exit")
}

func waitProcessExit(ctx context.Context, pid int, root string, role Role, limit time.Duration) bool {
	released := func() bool {
		if errors.Is(syscall.Kill(pid, 0), syscall.ESRCH) {
			return true
		}
		// An exited child awaiting waitpid(2) stays a zombie to kill(pid, 0),
		// but the kernel already closed its listener socket and role lock.
		dialCtx, cancel := context.WithTimeout(ctx, 100*time.Millisecond)
		defer cancel()
		c, err := dial(dialCtx, root, role)
		if err != nil {
			return true
		}
		_ = c.Close()
		return false
	}
	deadline := time.Now().Add(limit)
	for time.Now().Before(deadline) {
		if released() {
			return true
		}
		select {
		case <-ctx.Done():
			return false
		case <-time.After(25 * time.Millisecond):
		}
	}
	return released()
}

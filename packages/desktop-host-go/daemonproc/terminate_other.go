//go:build !windows

package daemonproc

import (
	"errors"
	"fmt"
	"syscall"
	"time"
)

// TerminateAndWait sends SIGTERM and returns without waiting, like the Rust Unix arm: the kernel lets a running
// binary be overwritten, and a daemon this process spawned stays a zombie until it exits, so a liveness poll here
// would not see it end. Callers that need the exit probe the daemon themselves. An already-gone pid is success.
func TerminateAndWait(pid uint32, _ time.Duration) error {
	if pid == 0 {
		return errors.New("refusing to terminate pid 0")
	}
	if err := syscall.Kill(int(pid), syscall.SIGTERM); err != nil && !errors.Is(err, syscall.ESRCH) {
		return fmt.Errorf("failed to terminate pid %d: %w", pid, err)
	}
	return nil
}

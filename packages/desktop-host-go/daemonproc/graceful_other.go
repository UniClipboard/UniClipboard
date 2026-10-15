//go:build !windows

package daemonproc

import (
	"errors"
	"time"
)

// gracefulStopFirst is false: SIGTERM, which TerminateAndWait sends, already runs the daemon's graceful shutdown.
const gracefulStopFirst = false

func waitProcessExit(uint32, time.Duration) (bool, error) {
	return false, errors.New("waiting for a process exit is only used on Windows")
}

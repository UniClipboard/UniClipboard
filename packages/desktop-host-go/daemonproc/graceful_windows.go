package daemonproc

import (
	"errors"
	"fmt"
	"time"

	"golang.org/x/sys/windows"
)

// gracefulStopFirst is true: TerminateProcess is the only stop on Windows and skips the daemon's shutdown task.
const gracefulStopFirst = true

// waitProcessExit waits up to timeout for the process to exit without terminating it. A process that is already
// gone has exited.
func waitProcessExit(pid uint32, timeout time.Duration) (bool, error) {
	h, err := windows.OpenProcess(windows.SYNCHRONIZE, false, pid)
	if err != nil {
		if errors.Is(err, windows.ERROR_INVALID_PARAMETER) { // no such process
			return true, nil
		}
		return false, fmt.Errorf("open daemon process %d: %w", pid, err)
	}
	defer windows.CloseHandle(h)
	event, err := windows.WaitForSingleObject(h, uint32(max(timeout, 0).Milliseconds()))
	if err != nil {
		return false, fmt.Errorf("wait for daemon process %d: %w", pid, err)
	}
	return event == uint32(windows.WAIT_OBJECT_0), nil
}

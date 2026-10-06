package daemonproc

import (
	"errors"
	"fmt"
	"time"

	"golang.org/x/sys/windows"
)

// TerminateAndWait terminates the process and blocks until its kernel object is signalled, i.e. until the
// executable's file lock is released and an installer may overwrite it (Rust: `terminate_and_wait_pid`). Polling
// a process list would race that release, and a console tool would flash a window. A process that is already gone
// is success, the goal state.
func TerminateAndWait(pid uint32, timeout time.Duration) error {
	if pid == 0 {
		return errors.New("refusing to terminate pid 0")
	}
	h, err := windows.OpenProcess(windows.PROCESS_TERMINATE|windows.SYNCHRONIZE, false, pid)
	if err != nil {
		if errors.Is(err, windows.ERROR_INVALID_PARAMETER) { // no such process
			return nil
		}
		return fmt.Errorf("open daemon process %d: %w", pid, err)
	}
	defer windows.CloseHandle(h)
	// The process may exit between open and terminate; the wait below settles it either way.
	_ = windows.TerminateProcess(h, 1)
	event, err := windows.WaitForSingleObject(h, uint32(timeout.Milliseconds()))
	if err != nil {
		return fmt.Errorf("wait for daemon process %d: %w", pid, err)
	}
	if event == uint32(windows.WAIT_OBJECT_0) {
		return nil
	}
	return fmt.Errorf("daemon pid %d did not exit within %s after TerminateProcess", pid, timeout)
}

package daemonproc

import (
	"os/exec"
	"syscall"

	"golang.org/x/sys/windows"
)

const (
	detachedProcess       = 0x00000008
	createNewProcessGroup = 0x00000200
)

func configureDetached(cmd *exec.Cmd) {
	cmd.SysProcAttr = &syscall.SysProcAttr{CreationFlags: detachedProcess | createNewProcessGroup}
}

// Terminate ends the daemon with Win32 TerminateProcess, like the Rust `terminate_local_daemon_pid`. The daemon is
// windowless (DETACHED_PROCESS), so a soft `taskkill /PID` has no window to ask and fails; shelling out to a console
// tool from the windowless GUI would also flash a console window.
func Terminate(pid uint32) bool {
	if pid == 0 {
		return false
	}
	h, err := windows.OpenProcess(windows.PROCESS_TERMINATE, false, pid)
	if err != nil {
		return false
	}
	defer windows.CloseHandle(h)
	return windows.TerminateProcess(h, 1) == nil
}

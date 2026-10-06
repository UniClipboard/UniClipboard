package daemonproc

import (
	"os/exec"
	"strconv"
	"syscall"
)

const (
	detachedProcess       = 0x00000008
	createNewProcessGroup = 0x00000200
	createNoWindow        = 0x08000000
)

func configureDetached(cmd *exec.Cmd) {
	cmd.SysProcAttr = &syscall.SysProcAttr{CreationFlags: detachedProcess | createNewProcessGroup}
}

// Terminate asks the daemon to exit through `taskkill /PID`, matching Rust.
func Terminate(pid uint32) bool {
	cmd := exec.Command("taskkill", "/PID", strconv.FormatUint(uint64(pid), 10))
	return cmd.Run() == nil
}

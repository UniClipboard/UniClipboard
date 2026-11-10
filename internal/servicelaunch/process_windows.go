package servicelaunch

import (
	"os/exec"
	"syscall"
)

// ConfigureProcess detaches a service from its launcher's console group.
func ConfigureProcess(cmd *exec.Cmd) {
	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true, CreationFlags: syscall.CREATE_NEW_PROCESS_GROUP}
}

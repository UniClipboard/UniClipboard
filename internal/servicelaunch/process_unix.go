//go:build !windows

package servicelaunch

import (
	"os/exec"
	"syscall"
)

// ConfigureProcess detaches a service from its launcher's terminal session.
// Ctrl+C or a terminal close must not stop a service other clients may still use.
func ConfigureProcess(cmd *exec.Cmd) {
	cmd.SysProcAttr = &syscall.SysProcAttr{Setsid: true}
}

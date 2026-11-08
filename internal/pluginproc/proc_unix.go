//go:build !windows

package pluginproc

import (
	"os/exec"
	"syscall"
)

func setProcAttrs(cmd *exec.Cmd) { cmd.SysProcAttr = &syscall.SysProcAttr{Setpgid: true} }

// killTree ends the plugin and everything it started in its process group.
func killTree(cmd *exec.Cmd) {
	if cmd.Process == nil {
		return
	}
	_ = syscall.Kill(-cmd.Process.Pid, syscall.SIGKILL)
	_ = cmd.Process.Kill()
}

func afterStart(*exec.Cmd) {}
func afterExit(*exec.Cmd)  {}

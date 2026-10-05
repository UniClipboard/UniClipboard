//go:build unix

package daemonproc

import (
	"os/exec"
	"syscall"
)

func configureDetached(cmd *exec.Cmd) {
	cmd.SysProcAttr = &syscall.SysProcAttr{Setsid: true}
}

// Terminate sends SIGTERM, matching the Rust `stop` command.
func Terminate(pid uint32) bool { return syscall.Kill(int(pid), syscall.SIGTERM) == nil }

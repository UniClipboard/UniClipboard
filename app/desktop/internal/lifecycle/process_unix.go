//go:build !windows

package lifecycle

import (
	"os/exec"
	"syscall"
)

func configureProcess(cmd *exec.Cmd) {
	// Logging out of the launching terminal must not terminate a lightweight service.
	cmd.SysProcAttr = &syscall.SysProcAttr{Setsid: true}
}

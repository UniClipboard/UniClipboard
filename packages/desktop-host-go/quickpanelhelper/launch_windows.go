package quickpanelhelper

import (
	"os/exec"
	"syscall"
)

// createNoWindow keeps the console-subsystem helper from opening a console window of its own; its standard
// streams are pipes, so nothing is lost.
const createNoWindow = 0x08000000

func configureProcess(cmd *exec.Cmd) {
	cmd.SysProcAttr = &syscall.SysProcAttr{CreationFlags: createNoWindow, HideWindow: true}
}

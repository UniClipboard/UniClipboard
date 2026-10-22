//go:build darwin || linux

package main

import (
	"os"
	"os/exec"
	"syscall"
)

func startUICommand(cmd *exec.Cmd, input, output *os.File) error {
	cmd.ExtraFiles = []*os.File{input, output}
	return cmd.Start()
}

func inheritedUIPipes() (*os.File, *os.File, error) {
	return openUIPipes(3, 4)
}

// Only the UI child inherits these descriptors. Do not leak them to commands it runs.
func prepareUIPipe(fd uintptr) error {
	syscall.CloseOnExec(int(fd))
	return syscall.SetNonblock(int(fd), true)
}

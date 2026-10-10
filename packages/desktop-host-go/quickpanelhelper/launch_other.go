//go:build !windows

package quickpanelhelper

import "os/exec"

func configureProcess(*exec.Cmd) {}

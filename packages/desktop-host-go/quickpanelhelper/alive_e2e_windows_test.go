//go:build e2e && windows

package quickpanelhelper

import (
	"os/exec"
	"strconv"
	"strings"
)

// alive reports whether a process with this pid exists (tasklist prints "INFO: No tasks ..." otherwise).
func alive(pid int) bool {
	out, err := exec.Command("tasklist", "/FI", "PID eq "+strconv.Itoa(pid), "/NH").Output()
	return err == nil && strings.Contains(string(out), " "+strconv.Itoa(pid)+" ")
}

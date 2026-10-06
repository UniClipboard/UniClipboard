//go:build unix

package commands

import (
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/exitcode"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
	"os"
	"syscall"
)

// Exec preserves the terminal, PID, signals and daemon exit status without a supervisor.
func foregroundProcess(exe string) int {
	if err := syscall.Exec(exe, []string{exe}, os.Environ()); err != nil {
		ui.RawStderr("Error: " + err.Error())
	}
	return exitcode.Error
}

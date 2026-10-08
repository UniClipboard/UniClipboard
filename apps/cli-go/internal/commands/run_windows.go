package commands

import (
	"errors"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/exitcode"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
	"os"
	"os/exec"
	"os/signal"
)

// The attached child shares the console and receives its console control events.
func foregroundProcess(exe string) int {
	// The console broadcasts Ctrl-C to both processes. Keep this parent alive
	// until the daemon finishes its own graceful shutdown.
	interrupts := make(chan os.Signal, 2)
	signal.Notify(interrupts, os.Interrupt)
	defer signal.Stop(interrupts)
	child := exec.Command(exe)
	child.Stdin, child.Stdout, child.Stderr = os.Stdin, os.Stdout, os.Stderr
	err := child.Run()
	if err == nil {
		return exitcode.Success
	}
	var exited *exec.ExitError
	if errors.As(err, &exited) {
		return exited.ExitCode()
	}
	ui.RawStderr("Error: " + err.Error())
	return exitcode.Error
}

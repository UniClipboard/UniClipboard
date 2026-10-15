// Command uniclip is the UniClipboard command-line interface.
package main

import (
	"os"
	"time"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/cli"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/commands"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/hostlog"
)

func main() {
	// Only the outcome is recorded: arguments may carry passphrases and are never logged.
	logger := hostlog.OpenDefault("uniclipboard-cli")
	started := time.Now()
	logger.Info("command started")
	code := cli.Execute(commands.Tree())
	if code == 0 {
		logger.Info("command finished exit_code=%d elapsed_ms=%d", code, time.Since(started).Milliseconds())
	} else {
		logger.Error("command failed exit_code=%d elapsed_ms=%d", code, time.Since(started).Milliseconds())
	}
	_ = logger.Close()
	os.Exit(code)
}

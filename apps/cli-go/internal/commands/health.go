package commands

import (
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/cli"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/exitcode"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonlife"
)

// runHealth reports whether this profile's daemon is healthy. It is read-only:
// it never starts, restarts or repairs a daemon.
func runHealth(ctx *cli.Context) int {
	if err := daemonlife.CheckHealthy(); err != nil {
		ui.RawStderr("Error: " + err.Error())
		return exitcode.Error
	}
	return printOrFail(map[string]string{"status": "ok"}, "Daemon healthy", ctx.JSON(), exitcode.Success)
}

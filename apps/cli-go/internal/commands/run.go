package commands

import (
	"fmt"
	"os"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/cli"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/exitcode"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonlife"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonproc"
)

// requireAbsent refuses incumbents even when their HTTP endpoint is unhealthy.
func requireAbsent() error {
	meta, err := daemonproc.ReadPidMetadata()
	if err != nil {
		return err
	}
	if meta != nil && daemonproc.IsActiveDaemon(meta.PID) {
		return fmt.Errorf("daemon already running for this profile (pid %d); leave its owner running or stop it explicitly before run/service start", meta.PID)
	}
	live, err := daemonproc.ConnPointsToLiveDaemon()
	if err != nil {
		return err
	}
	if live {
		return fmt.Errorf("an existing daemon owns this profile; refusing to start another")
	}
	outcome, err := daemonlife.Probe()
	if err != nil {
		return err
	}
	if outcome.Kind != daemonlife.Absent {
		return fmt.Errorf("an existing daemon endpoint owns this profile; refusing to start another")
	}
	return nil
}

func runForeground(ctx *cli.Context) int {
	configureRunMode(ctx.Bool("server"))
	if err := requireAbsent(); err != nil {
		ui.RawStderr("Error: " + err.Error())
		return exitcode.Error
	}
	exe, err := daemonproc.ResolveDaemonExe()
	if err != nil {
		ui.RawStderr("Error: " + err.Error())
		return exitcode.Error
	}
	os.Setenv("UC_DAEMON_SPAWN_ORIGIN", "cli")
	os.Setenv(daemonproc.NoTakeoverEnv, "1")
	os.Unsetenv("UC_DISABLE_DAEMON_SINGLE_INSTANCE")
	return foregroundProcess(exe)
}

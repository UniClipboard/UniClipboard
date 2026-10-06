package commands

import (
	"errors"
	"fmt"
	"os"
	"os/exec"
	"time"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/cli"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/exitcode"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/localdaemon"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/output"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/session"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonlife"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonproc"
)

type startOutput struct {
	Status string  `json:"status"`
	PID    *uint32 `json:"pid"`
}

func (o startOutput) human() string {
	switch {
	case o.Status == "started" && o.PID != nil:
		return fmt.Sprintf("Daemon started (pid %d)", *o.PID)
	case o.Status == "already_running" && o.PID != nil:
		return fmt.Sprintf("Daemon already running (pid %d)", *o.PID)
	case o.Status == "started":
		return "Daemon started"
	case o.Status == "already_running":
		return "Daemon already running"
	case o.PID != nil:
		return fmt.Sprintf("Daemon %s (pid %d)", o.Status, *o.PID)
	default:
		return "Daemon " + o.Status
	}
}

func printOrFail(value any, human string, asJSON bool, code int) int {
	if err := output.PrintResult(value, human, asJSON); err != nil {
		ui.RawStderr("Error: " + err.Error())
		return exitcode.Error
	}
	return code
}

func configureRunMode(server bool) {
	if server {
		os.Setenv(daemonproc.RunModeEnv, daemonproc.RunModeServer)
	} else {
		os.Unsetenv(daemonproc.RunModeEnv)
	}
}

func runStart(ctx *cli.Context) int {
	ui.Warn("`uniclip start` is deprecated; use `uniclip run` for foreground or `uniclip service start` for background. This alias retains its existing behavior during the compatibility period.")
	server := ctx.Bool("server")
	configureRunMode(server)
	if ctx.Bool("foreground") {
		return startForeground(ctx.JSON())
	}
	lease, _, err := session.ConnectSetupWithLease(true)
	if err != nil {
		return session.ExitCode(err)
	}
	code, done := checkSetupComplete(ctx.JSON())
	lease.Release()
	if done {
		return code
	}
	configureRunMode(server)
	target := daemonlife.ResidencyStandalone
	if server {
		target = daemonlife.ResidencyServerHeadless
	}
	sess, err := localdaemon.EnsureOrPromote(target)
	if err != nil {
		ui.RawStderr("Error: " + err.Error())
		return exitcode.Error
	}
	out := startOutput{Status: "already_running", PID: readPid()}
	if sess.Spawned {
		out.Status = "started"
	}
	return printOrFail(out, out.human(), ctx.JSON(), exitcode.Success)
}

func readPid() *uint32 {
	meta, err := daemonproc.ReadPidMetadata()
	if err != nil || meta == nil {
		return nil
	}
	return &meta.PID
}

// checkSetupComplete returns (code, true) when start must stop because the
// profile has no space yet or setup state could not be read.
func checkSetupComplete(asJSON bool) (int, bool) {
	complete, err := session.WaitForSetupComplete()
	if err != nil {
		ui.Error("Failed to read setup state: " + err.Error())
		return exitcode.Error, true
	}
	if complete {
		return 0, false
	}
	if asJSON {
		output.PrintResult(startOutput{Status: "setup_required"}, "", true)
	} else {
		ui.Error("setup not complete. Run `uniclip space init` (new Space) or `uniclip space join` (existing Space) first, then retry `start`.")
	}
	return exitcode.Error, true
}

func startForeground(asJSON bool) int {
	outcome, err := daemonlife.ProbeForReuse(daemonlife.StartupTimeout)
	if err != nil {
		ui.Error("Failed to probe local daemon: " + err.Error())
		return exitcode.DaemonUnreachable
	}
	switch outcome.Kind {
	case daemonlife.Compatible:
		if code, done := checkSetupComplete(asJSON); done {
			return code
		}
		out := startOutput{Status: "already_running", PID: readPid()}
		return printOrFail(out, out.human(), asJSON, exitcode.Success)
	case daemonlife.Incompatible:
		ui.RawStderr("Error: " + daemonlife.IncompatibleError(outcome).Error())
		return exitcode.Error
	}
	exe, err := daemonproc.ResolveDaemonExe()
	if err != nil {
		ui.RawStderr("Error: " + err.Error())
		return exitcode.Error
	}
	if !asJSON {
		fmt.Println("Starting daemon in foreground... (press Ctrl+C to stop)")
	}
	child := exec.Command(exe)
	child.Stdin = nil
	child.Stdout = os.Stdout
	if asJSON {
		child.Stdout = os.Stderr
	}
	child.Stderr = os.Stderr
	if err := child.Start(); err != nil {
		ui.RawStderr("Error: failed to spawn daemon: " + err.Error())
		return exitcode.Error
	}
	stop := func() {
		child.Process.Kill()
		child.Wait()
	}
	if err := daemonlife.WaitForRunningDaemon(); err != nil {
		stop()
		ui.Error("Failed to start daemon: " + err.Error())
		return exitcode.Error
	}
	if code, done := checkSetupComplete(asJSON); done {
		stop()
		return code
	}
	if err := child.Wait(); err != nil {
		var exitErr *exec.ExitError
		if !errors.As(err, &exitErr) {
			ui.RawStderr("Error: failed to wait for daemon process: " + err.Error())
			return exitcode.Error
		}
	}
	return exitcode.Success
}

type stopOutput struct {
	Status string  `json:"status"`
	PID    *uint32 `json:"pid,omitempty"`
}

func (o stopOutput) human() string {
	switch {
	case o.Status == "stopped":
		return "Daemon stopped"
	case o.Status == "not_running":
		return "Daemon is not running"
	case o.Status == "managed_by_gui" && o.PID != nil:
		return fmt.Sprintf("Daemon (pid %d) is running inside the UniClipboard GUI. Quit the GUI from its tray menu instead — `uniclip stop` won't kill it.", *o.PID)
	case o.Status == "managed_by_gui":
		return "Daemon is running inside the UniClipboard GUI. Quit the GUI from its tray menu instead."
	case o.PID != nil:
		return fmt.Sprintf("Daemon %s (pid %d)", o.Status, *o.PID)
	default:
		return "Daemon " + o.Status
	}
}

func runStop(ctx *cli.Context) int {
	asJSON := ctx.JSON()
	meta, err := daemonproc.ReadPidMetadata()
	if err != nil {
		ui.RawStderr("Error: failed to read daemon PID file: " + err.Error())
		return exitcode.Error
	}
	notRunning := stopOutput{Status: "not_running"}
	if meta == nil || !daemonproc.IsActiveDaemon(meta.PID) {
		return printOrFail(notRunning, notRunning.human(), asJSON, exitcode.Success)
	}
	pid := meta.PID
	if meta.Mode == "in_process" {
		out := stopOutput{Status: "managed_by_gui", PID: &pid}
		return printOrFail(out, out.human(), asJSON, exitcode.Error)
	}
	if !daemonproc.Terminate(pid) {
		ui.RawStderr(fmt.Sprintf("Error: failed to send stop signal to daemon (pid %d)", pid))
		return exitcode.Error
	}
	const stopTimeout = 10 * time.Second
	deadline := time.Now().Add(stopTimeout)
	for {
		time.Sleep(200 * time.Millisecond)
		if !daemonproc.IsActiveDaemon(pid) {
			break
		}
		if !time.Now().Before(deadline) {
			ui.RawStderr(fmt.Sprintf("Warning: daemon (pid %d) did not stop within %ds. You may need to terminate it manually.", pid, int(stopTimeout.Seconds())))
			return exitcode.Error
		}
	}
	out := stopOutput{Status: "stopped", PID: &pid}
	return printOrFail(out, out.human(), asJSON, exitcode.Success)
}

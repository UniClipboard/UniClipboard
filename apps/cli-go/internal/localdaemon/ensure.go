package localdaemon

import (
	"os"
	"time"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonlife"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonproc"
)

// Session reports how a daemon was obtained.
type Session struct {
	BaseURL string
	Spawned bool
}

func resolvedSession(spawned bool) (Session, error) {
	conn, err := daemonproc.ReadConnFile()
	if err != nil {
		return Session{}, &daemonlife.Error{Kind: daemonlife.ErrResolveAddress, Err: err}
	}
	if conn == nil {
		return Session{}, &daemonlife.Error{Kind: daemonlife.ErrResolveAddress, Err: errDaemonConnMissing}
	}
	return Session{BaseURL: conn.BaseURL(), Spawned: spawned}, nil
}

type stringError string

func (e stringError) Error() string { return string(e) }

const errDaemonConnMissing = stringError("daemon connection file not found (is the daemon running?)")

// spawnAndWait spawns a detached daemon and waits until it is healthy.
func spawnAndWait(timeout time.Duration) (Session, error) {
	spinner := ui.NewSpinner("Starting local daemon…")
	if err := daemonproc.SpawnDetachedDaemon("cli"); err != nil {
		spinner.FinishError("Failed to spawn local daemon")
		return Session{}, &daemonlife.Error{Kind: daemonlife.ErrSpawn, Err: err}
	}
	if err := daemonlife.WaitHealthy(timeout, ""); err != nil {
		spinner.FinishError("Local daemon failed to start")
		return Session{}, err
	}
	spinner.FinishSuccess("Local daemon ready")
	return resolvedSession(true)
}

// SpawnOneshotAndWait spawns a self-terminating oneshot daemon. The child
// inherits UC_DAEMON_RUN_MODE=oneshot from this process, as in Rust.
func SpawnOneshotAndWait(timeout time.Duration) (Session, error) {
	os.Setenv(daemonproc.RunModeEnv, daemonproc.RunModeOneshot)
	return spawnAndWait(timeout)
}

// EnsureOrPromote is the background `start` path: reuse a persistent daemon,
// promote a oneshot daemon to target, report an incompatible one, or spawn.
func EnsureOrPromote(target string) (Session, error) {
	outcome, err := daemonlife.ProbeForReuse(daemonlife.StartupTimeout)
	if err != nil {
		return Session{}, err
	}
	switch outcome.Kind {
	case daemonlife.Compatible:
		if outcome.Health.Residency == daemonlife.ResidencyOneshot {
			return promote(target)
		}
		return resolvedSession(false)
	case daemonlife.Incompatible:
		return Session{}, daemonlife.IncompatibleError(outcome)
	default:
		return spawnAndWait(daemonlife.StartupTimeout)
	}
}

func promote(target string) (Session, error) {
	spinner := ui.NewSpinner("Promoting daemon…")
	if err := daemonlife.PromoteOneshot(target, "cli"); err != nil {
		spinner.FinishError("Failed to promote the daemon")
		return Session{}, err
	}
	spinner.FinishSuccess("Daemon promoted")
	return resolvedSession(true)
}

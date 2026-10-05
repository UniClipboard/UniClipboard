package localdaemon

import (
	"context"
	"net/http"
	"os"
	"time"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/daemonclient"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/daemonproc"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
)

// Session reports how a daemon was obtained.
type Session struct {
	BaseURL string
	Spawned bool
}

func resolvedSession(spawned bool) (Session, error) {
	conn, err := daemonproc.ReadConnFile()
	if err != nil {
		return Session{}, &Error{Kind: ErrResolveAddress, Err: err}
	}
	if conn == nil {
		return Session{}, &Error{Kind: ErrResolveAddress, Err: errDaemonConnMissing}
	}
	return Session{BaseURL: conn.BaseURL(), Spawned: spawned}, nil
}

type stringError string

func (e stringError) Error() string { return string(e) }

const errDaemonConnMissing = stringError("daemon connection file not found (is the daemon running?)")

// spawnAndWait spawns a detached daemon and waits until it is healthy.
func spawnAndWait(timeout time.Duration) (Session, error) {
	spinner := ui.NewSpinner("Starting local daemon…")
	if err := daemonproc.SpawnDetachedDaemon(); err != nil {
		spinner.FinishError("Failed to spawn local daemon")
		return Session{}, &Error{Kind: ErrSpawn, Err: err}
	}
	if err := waitHealthy(timeout, ""); err != nil {
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
	outcome, err := ProbeForReuse(StartupTimeout)
	if err != nil {
		return Session{}, err
	}
	switch outcome.Kind {
	case Compatible:
		if outcome.Health.Residency == ResidencyOneshot {
			return promote(target)
		}
		return resolvedSession(false)
	case Incompatible:
		return Session{}, IncompatibleError(outcome)
	default:
		return spawnAndWait(StartupTimeout)
	}
}

func promote(target string) (Session, error) {
	spinner := ui.NewSpinner("Promoting daemon…")
	client, err := daemonclient.FromEnv()
	if err != nil {
		spinner.FinishError("Failed to reach local daemon for promotion")
		return Session{}, &Error{Kind: ErrPromoteRestart, Err: err}
	}
	err = client.Enveloped(context.Background(), daemonclient.Request{
		Method: http.MethodPost, Path: "/lifecycle/restart", JSON: map[string]string{"targetMode": target},
	}, nil)
	if err != nil {
		spinner.FinishError("Daemon rejected the promotion request")
		return Session{}, &Error{Kind: ErrPromoteRestart, Err: err}
	}
	if err := waitAbsent(promoteDrainTimeout); err != nil {
		spinner.FinishError("Daemon did not drain for promotion")
		return Session{}, err
	}
	if err := daemonproc.SpawnDetachedDaemon(); err != nil {
		spinner.FinishError("Failed to spawn promoted daemon")
		return Session{}, &Error{Kind: ErrSpawn, Err: err}
	}
	if err := waitHealthy(StartupTimeout, target); err != nil {
		spinner.FinishError("Promoted daemon failed to start")
		return Session{}, err
	}
	spinner.FinishSuccess("Daemon promoted")
	return resolvedSession(true)
}

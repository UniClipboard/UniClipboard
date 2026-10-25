package daemonlife

import (
	"errors"
	"fmt"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonproc"
)

// ErrNotHealthy marks a read-only health verdict of "not healthy", as opposed
// to a failure to evaluate the check.
var ErrNotHealthy = errors.New("daemon is not healthy")

// CheckHealthy is a strictly read-only liveness check for orchestrators such
// as container healthchecks. It never spawns, restarts, repairs or takes over
// a daemon. It succeeds only when the profile's `daemon.conn` names a live
// daemon process that answers `/health` as compatible, and that same record is
// still published after the answer (so a daemon replaced in flight is not
// mistaken for the one that answered).
func CheckHealthy() error {
	conn, err := daemonproc.ReadConnFile()
	if err != nil {
		return fmt.Errorf("%w: %v", ErrNotHealthy, err)
	}
	if conn == nil {
		return fmt.Errorf("%w: no daemon.conn published for this profile", ErrNotHealthy)
	}
	if !daemonproc.IsActiveDaemon(conn.PID) {
		return fmt.Errorf("%w: daemon.conn names pid %d, which is not a running daemon", ErrNotHealthy, conn.PID)
	}
	outcome, err := probeAt(conn.BaseURL())
	if err != nil {
		return fmt.Errorf("%w: %v", ErrNotHealthy, err)
	}
	switch outcome.Kind {
	case Absent:
		return fmt.Errorf("%w: daemon pid %d does not answer on %s", ErrNotHealthy, conn.PID, conn.BaseURL())
	case Incompatible:
		return fmt.Errorf("%w: %s", ErrNotHealthy, outcome.Details)
	}
	current, err := daemonproc.ReadConnFile()
	if err != nil || current == nil || *current != *conn || !daemonproc.IsActiveDaemon(conn.PID) {
		return fmt.Errorf("%w: daemon.conn changed during the check", ErrNotHealthy)
	}
	return nil
}

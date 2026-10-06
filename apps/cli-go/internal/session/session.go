// Package session mirrors the Rust CLI's `app_session`: obtaining a daemon
// client (reusing, or spawning a oneshot daemon), holding the control lease,
// and setup-state checks. On failure the functions have already printed the
// user-facing error and return the exit code to use.
package session

import (
	"context"
	"errors"
	"fmt"
	"os"
	"strings"
	"time"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/exitcode"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/localdaemon"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/buildinfo"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonlife"
)

// Failed carries the exit code of an already-reported failure.
type Failed struct{ Code int }

func (f *Failed) Error() string { return fmt.Sprintf("exit %d", f.Code) }

func fail(code int) error { return &Failed{Code: code} }

// ExitCode returns the code carried by a session failure.
func ExitCode(err error) int {
	var f *Failed
	if errors.As(err, &f) {
		return f.Code
	}
	return exitcode.Error
}

// ConnectOrSpawnOneshot reuses a compatible daemon or spawns a oneshot one.
// deadline, when non-zero, bounds the whole wait (used by `send`).
func ConnectOrSpawnOneshot(deadline time.Time) (*daemonclient.Client, error) {
	remaining := func() time.Duration {
		if deadline.IsZero() {
			return daemonlife.StartupTimeout
		}
		if d := time.Until(deadline); d > 0 {
			return d
		}
		return 0
	}
	reportTimeout := func(err error) error {
		ui.Error(err.Error())
		var le *daemonlife.Error
		if !deadline.IsZero() && errors.As(err, &le) && le.Kind == daemonlife.ErrStartupTimeout {
			ui.Warn("The daemon may still be starting. Retry, raise --connect-timeout, or run `uniclip start` first.")
			return fail(exitcode.DaemonUnreachable)
		}
		return fail(exitcode.Error)
	}
	outcome, err := daemonlife.ProbeForReuse(remaining())
	if err != nil {
		var le *daemonlife.Error
		if errors.As(err, &le) && le.Kind == daemonlife.ErrStartupTimeout && !deadline.IsZero() {
			return nil, reportTimeout(err)
		}
		ui.Error("Failed to probe local daemon: " + err.Error())
		return nil, fail(exitcode.DaemonUnreachable)
	}
	switch outcome.Kind {
	case daemonlife.Compatible:
		return buildClient(true)
	case daemonlife.Incompatible:
		ui.Error(daemonlife.IncompatibleError(outcome).Error())
		return nil, fail(exitcode.DaemonUnreachable)
	}
	if _, err := localdaemon.SpawnOneshotAndWait(remaining()); err != nil {
		return nil, reportTimeout(err)
	}
	client, err := daemonclient.FromEnv()
	if err != nil {
		ui.Error("Failed to connect to daemon: " + err.Error())
		return nil, fail(exitcode.Error)
	}
	var recovery struct {
		BackgroundReady bool `json:"backgroundReady"`
	}
	if err := client.Get(context.Background(), "/encryption/recovery", &recovery); err != nil {
		ui.Error("Failed to read profile recovery state: " + err.Error())
		return nil, fail(exitcode.Error)
	}
	if !recovery.BackgroundReady {
		return client, nil
	}
	complete, err := IsSetupComplete(client)
	if err != nil {
		ui.Error("Failed to read setup state: " + err.Error())
		return nil, fail(exitcode.Error)
	}
	if !complete {
		ui.Error("No space on this profile; run `uniclip space init` or `uniclip space join` first.")
		return nil, fail(exitcode.Error)
	}
	return client, nil
}

// ConnectWithLease connects (spawning a oneshot daemon if needed) and holds
// the control lease for the life of the command.
func ConnectWithLease() (*daemonclient.Lease, *daemonclient.Client, error) {
	client, err := ConnectOrSpawnOneshot(time.Time{})
	if err != nil {
		return nil, nil, err
	}
	lease, err := client.HoldLease(context.Background())
	if err != nil {
		ui.Error("Failed to hold daemon session lease: " + err.Error())
		return nil, nil, fail(exitcode.Error)
	}
	return lease, client, nil
}

// ConnectSetupWithLease connects for setup flows, which may attach to a
// version-matched degraded daemon.
func ConnectSetupWithLease(reportErrors bool) (*daemonclient.Lease, *daemonclient.Client, error) {
	client, err := EnsureDaemonForSetup(reportErrors)
	if err != nil {
		return nil, nil, err
	}
	lease, err := client.HoldLease(context.Background())
	if err != nil {
		report("Failed to hold daemon session lease: "+err.Error(), reportErrors)
		return nil, nil, fail(exitcode.Error)
	}
	return lease, client, nil
}

func report(message string, reportErrors bool) {
	if reportErrors {
		ui.Error(message)
	}
}

func buildClient(reportErrors bool) (*daemonclient.Client, error) {
	client, err := daemonclient.FromEnv()
	if err != nil {
		report("Daemon is running but failed to connect: "+err.Error(), reportErrors)
		return nil, fail(exitcode.Error)
	}
	return client, nil
}

// EnsureDaemonForSetup reuses, attaches to a matching degraded daemon, or
// spawns a oneshot daemon for setup commands.
func EnsureDaemonForSetup(reportErrors bool) (*daemonclient.Client, error) {
	outcome, err := daemonlife.ProbeForReuse(daemonlife.StartupTimeout)
	if err != nil {
		report("Failed to probe local daemon: "+err.Error(), reportErrors)
		return nil, fail(exitcode.DaemonUnreachable)
	}
	switch {
	case outcome.Kind == daemonlife.Compatible:
		return buildClient(reportErrors)
	case setupControlContractMatches(outcome):
		return buildClient(reportErrors)
	case outcome.Kind == daemonlife.Incompatible:
		report(daemonlife.IncompatibleError(outcome).Error(), reportErrors)
		return nil, fail(exitcode.DaemonUnreachable)
	}
	if _, err := localdaemon.SpawnOneshotAndWait(daemonlife.StartupTimeout); err != nil {
		report(err.Error(), reportErrors)
		return nil, fail(exitcode.Error)
	}
	return buildClient(reportErrors)
}

func setupControlContractMatches(o daemonlife.Outcome) bool {
	return o.Kind == daemonlife.Incompatible && o.Details == daemonlife.DegradedDetails &&
		o.ObservedVersion != nil && *o.ObservedVersion == buildinfo.PackageVersion &&
		o.ObservedAPIVersion != nil && *o.ObservedAPIVersion == buildinfo.DaemonAPIRevision
}

// WaitAndReconnect waits for a restarted daemon to become compatible.
func WaitAndReconnect(timeout time.Duration) (*daemonclient.Client, error) {
	deadline := time.Now().Add(timeout)
	for {
		if outcome, err := daemonlife.Probe(); err == nil && outcome.Kind == daemonlife.Compatible {
			return buildClient(true)
		}
		if !time.Now().Before(deadline) {
			ui.Error("Timed out waiting for daemon to restart.")
			return nil, fail(exitcode.DaemonUnreachable)
		}
		time.Sleep(200 * time.Millisecond)
	}
}

// IsSetupComplete asks the daemon whether this profile has a space.
func IsSetupComplete(client *daemonclient.Client) (bool, error) {
	var state struct {
		HasCompleted bool `json:"hasCompleted"`
	}
	if err := client.Get(context.Background(), "/v2/setup/state", &state); err != nil {
		return false, err
	}
	return state.HasCompleted, nil
}

// WaitForSetupComplete retries while the daemon reports runtime_unavailable
// (the endpoint is up before profile recovery finished).
func WaitForSetupComplete() (bool, error) {
	deadline := time.Now().Add(10 * time.Second)
	for {
		client, err := daemonclient.FromEnv()
		if err != nil {
			return false, err
		}
		complete, err := IsSetupComplete(client)
		if err == nil {
			return complete, nil
		}
		if daemonclient.ErrorCode(err) == "runtime_unavailable" && time.Now().Before(deadline) {
			time.Sleep(100 * time.Millisecond)
			continue
		}
		return false, err
	}
}

// DefaultDeviceName is the hostname, plus ` (profile)` when a profile is set.
func DefaultDeviceName() (string, bool) {
	host, err := os.Hostname()
	if err != nil {
		return "", false
	}
	host = strings.TrimSpace(host)
	if host == "" {
		return "", false
	}
	if profile := os.Getenv("UC_PROFILE"); profile != "" {
		return fmt.Sprintf("%s (%s)", host, profile), true
	}
	return host, true
}

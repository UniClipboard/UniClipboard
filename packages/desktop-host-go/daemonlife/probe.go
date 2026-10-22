// Package daemonlife mirrors the Rust CLI's `local_daemon` module: health
// classification, reuse probing, detached spawn, and oneshot promotion.
package daemonlife

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/buildinfo"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonproc"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/errctx"
)

// Timing contract from `uc-daemon-process::timing`.
const (
	probeTimeout        = 2 * time.Second
	pollInterval        = 200 * time.Millisecond
	predecessorRelease  = 2*5*time.Second + 5*time.Second // 2 * SHUTDOWN_JOIN_TIMEOUT + IROH_TEARDOWN_MARGIN
	lockAcquireDeadline = 2 * predecessorRelease
	// StartupTimeout is DAEMON_STARTUP_TIMEOUT.
	StartupTimeout      = lockAcquireDeadline + 15*time.Second
	PromoteDrainTimeout = 2 * 30 * time.Second
)

// Residency values reported by `/health`.
const (
	ResidencyStandalone     = "standalone"
	ResidencyServerHeadless = "serverHeadless"
	ResidencyOneshot        = "oneshot"
)

// DegradedDetails is DEGRADED_HEALTH_INCOMPATIBILITY_DETAILS.
const DegradedDetails = "daemon reported unhealthy status degraded"

// Health is the `/health` payload.
type Health struct {
	Status         string `json:"status"`
	PackageVersion string `json:"packageVersion"`
	APIRevision    string `json:"apiRevision"`
	Residency      string `json:"residency"`
}

// OutcomeKind classifies a probe.
type OutcomeKind int

const (
	Absent OutcomeKind = iota
	Compatible
	Incompatible
)

// Outcome mirrors `ProbeOutcome`.
type Outcome struct {
	Kind               OutcomeKind
	Health             Health
	Details            string
	ObservedVersion    *string
	ObservedAPIVersion *string
}

func classify(h Health) Outcome {
	version, revision := h.PackageVersion, h.APIRevision
	incompatible := func(details string) Outcome {
		return Outcome{Kind: Incompatible, Details: details, ObservedVersion: &version, ObservedAPIVersion: &revision}
	}
	if h.Status != "ok" && h.Status != "recovery_required" {
		if h.Status == "degraded" {
			return incompatible(DegradedDetails)
		}
		return incompatible("daemon reported unhealthy status " + h.Status)
	}
	if strings.TrimSpace(version) == "" {
		return incompatible("daemon health response missing packageVersion")
	}
	if strings.TrimSpace(revision) == "" {
		return incompatible("daemon health response missing apiRevision")
	}
	if version != buildinfo.PackageVersion {
		return incompatible(fmt.Sprintf("daemon packageVersion %s does not match shell packageVersion %s", version, buildinfo.PackageVersion))
	}
	if revision != buildinfo.DaemonAPIRevision {
		return incompatible(fmt.Sprintf("daemon apiRevision %s does not match required %s", revision, buildinfo.DaemonAPIRevision))
	}
	return Outcome{Kind: Compatible, Health: h}
}

var probeClient = daemonclient.NewLocalHTTPClient(probeTimeout)

// Probe classifies the daemon bound to this profile without spawning one.
func Probe() (Outcome, error) {
	conn, err := daemonproc.ReadConnFile()
	if err != nil {
		return Outcome{}, &Error{Kind: ErrResolveAddress, Err: errctx.Wrap("failed to read daemon connection file", err)}
	}
	if conn == nil {
		return Outcome{Kind: Absent}, nil
	}
	return probeAt(conn.BaseURL())
}

func probeAt(baseURL string) (Outcome, error) {
	resp, err := probeClient.Get(baseURL + "/health")
	if err != nil {
		var netErr net.Error
		var opErr *net.OpError
		if (errors.As(err, &opErr) && opErr.Op == "dial") || (errors.As(err, &netErr) && netErr.Timeout()) {
			return Outcome{Kind: Absent}, nil
		}
		return Outcome{}, &Error{Kind: ErrProbe, Err: errctx.Wrap("daemon health probe request failed", err)}
	}
	defer resp.Body.Close()
	if resp.StatusCode < 200 || resp.StatusCode > 299 {
		return Outcome{Kind: Incompatible, Details: "daemon health probe returned HTTP " + daemonclient.StatusText(resp.StatusCode)}, nil
	}
	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return Outcome{}, &Error{Kind: ErrProbe, Err: errctx.Wrap("failed to read daemon health response body", err)}
	}
	var env struct {
		Data *Health `json:"data"`
	}
	if err := json.Unmarshal(body, &env); err != nil || env.Data == nil {
		detail := "missing field `data`"
		if err != nil {
			detail = err.Error()
		}
		return Outcome{Kind: Incompatible, Details: "failed to decode daemon health response: " + detail}, nil
	}
	if env.Data.Residency == "" {
		env.Data.Residency = ResidencyStandalone
	}
	return classify(*env.Data), nil
}

// ProbeForReuse treats a live `daemon.conn` incumbent that has not answered
// `/health` yet as starting, and waits up to timeout instead of reporting
// Absent (which would let a caller spawn a competing daemon).
func ProbeForReuse(timeout time.Duration) (Outcome, error) {
	deadline := time.Now().Add(timeout)
	for {
		outcome, err := Probe()
		if err != nil {
			return Outcome{}, err
		}
		if outcome.Kind != Absent {
			return outcome, nil
		}
		live, err := daemonproc.ProfileHasLiveDaemon()
		if err != nil {
			return Outcome{}, &Error{Kind: ErrResolveAddress, Err: errctx.Wrap("failed to read daemon connection file", err)}
		}
		if !live {
			return outcome, nil
		}
		if err := startupFailure(nil); err != nil {
			return Outcome{}, err
		}
		if !time.Now().Before(deadline) {
			return Outcome{}, startupTimeout(timeout)
		}
		time.Sleep(pollInterval)
	}
}

// ErrorKind enumerates `LocalDaemonError` variants.
type ErrorKind int

const (
	ErrResolveAddress ErrorKind = iota
	ErrProbe
	ErrSpawn
	ErrStartupTimeout
	ErrIncompatible
	ErrPromoteRestart
	ErrPromoteDrainTimeout
	ErrStartupFailed
)

// Error mirrors `LocalDaemonError` and its Display text.
type Error struct {
	Kind            ErrorKind
	Err             error
	Timeout         time.Duration
	Profile         string
	BaseURL         string
	Details         string
	ObservedVersion *string
	Newer           bool
	Retryable       bool
}

func (e *Error) Error() string {
	profile := e.Profile
	if profile == "" {
		profile = "default"
	}
	switch e.Kind {
	case ErrResolveAddress:
		return fmt.Sprintf("failed to resolve profile-aware local daemon address: %v", e.Err)
	case ErrProbe:
		return fmt.Sprintf("failed to probe local daemon health for setup: %v", e.Err)
	case ErrSpawn:
		// Rust converts SpawnDaemonError into its inner error, dropping the
		// SpawnDaemonError Display prefix.
		var spawnErr *daemonproc.SpawnError
		if errors.As(e.Err, &spawnErr) {
			if spawnErr.ResolveBinary {
				return fmt.Sprintf("failed to resolve CLI executable for daemon spawn: %v", spawnErr.Err)
			}
			return fmt.Sprintf("failed to spawn daemon process: %v", spawnErr.Err)
		}
		return fmt.Sprintf("failed to spawn daemon process: %v", e.Err)
	case ErrStartupTimeout:
		location := e.BaseURL
		if location == "" {
			location = "no business endpoint published"
		}
		return fmt.Sprintf("local daemon did not become healthy within %dms for profile %s (%s)", e.Timeout.Milliseconds(), profile, location)
	case ErrStartupFailed:
		return fmt.Sprintf("local daemon startup failed for profile %s: %s", profile, e.Details)
	case ErrIncompatible:
		observed := "unknown"
		if e.ObservedVersion != nil {
			observed = *e.ObservedVersion
		}
		if e.Newer {
			return fmt.Sprintf("a newer daemon (version %s) is already running for this profile; this CLI is %s — refusing to act against a newer daemon. Re-upgrade the CLI, or restart the daemon to converge (%s)", observed, buildinfo.PackageVersion, e.Details)
		}
		return fmt.Sprintf("an incompatible daemon (version %s) is already running for this profile; this CLI expects %s. Stop it with `uniclip stop` and restart, or upgrade the daemon to match (%s)", observed, buildinfo.PackageVersion, e.Details)
	case ErrPromoteRestart:
		return fmt.Sprintf("failed to promote the transient local daemon to a persistent one: %v", e.Err)
	default:
		return fmt.Sprintf("the transient local daemon did not drain and exit within %dms for profile %s at %s; promotion aborted (the old daemon is left running)", e.Timeout.Milliseconds(), profile, e.BaseURL)
	}
}

func (e *Error) Unwrap() error { return e.Err }

func currentBaseURL() string {
	conn, err := daemonproc.ReadConnFile()
	if err != nil || conn == nil {
		return ""
	}
	return conn.BaseURL()
}

func startupTimeout(timeout time.Duration) *Error {
	return &Error{Kind: ErrStartupTimeout, Timeout: timeout, Profile: os.Getenv("UC_PROFILE"), BaseURL: currentBaseURL()}
}

// startupFailure reads the independent service before the business API exists.
func startupFailure(attempt *daemonproc.StartupAttempt) error {
	ctx, cancel := context.WithTimeout(context.Background(), probeTimeout)
	defer cancel()
	conn, err := daemonproc.ReadStartupConnFile()
	if err != nil {
		return &Error{Kind: ErrProbe, Err: err}
	}
	if conn == nil || (attempt != nil && (conn.PID != attempt.PID() || conn.InstanceID != attempt.InstanceID())) {
		return nil
	}
	body, err := daemonclient.ReadStartupStatusAt(ctx, *conn)
	if err != nil {
		return &Error{Kind: ErrProbe, Err: errctx.Wrap("failed to read daemon startup status", err)}
	}
	if len(body) == 0 {
		return nil
	}
	var status struct {
		ServiceFailed bool `json:"service_failed"`
		Progress      struct {
			State          string `json:"state"`
			AllowedActions struct {
				Retry bool `json:"retry"`
			} `json:"allowed_actions"`
			Upgrade *struct {
				CurrentStep string `json:"current_step"`
			} `json:"upgrade"`
			Failure *struct {
				Reason    string `json:"reason"`
				Retryable bool   `json:"retryable"`
			} `json:"failure"`
		} `json:"progress"`
	}
	if err := json.Unmarshal(body, &status); err != nil {
		return &Error{Kind: ErrProbe, Err: errctx.Wrap("failed to decode daemon startup status", err)}
	}
	if !status.ServiceFailed && status.Progress.State != "failed" && status.Progress.State != "interrupted" {
		return nil
	}
	details := "source=daemon.startup, state=" + status.Progress.State
	if status.Progress.Upgrade != nil && status.Progress.Upgrade.CurrentStep != "" {
		details += ", step=" + status.Progress.Upgrade.CurrentStep
	}
	if status.Progress.Failure != nil {
		details += fmt.Sprintf(", reason=%s, retryable=%t", status.Progress.Failure.Reason, status.Progress.Failure.Retryable)
	}
	return &Error{Kind: ErrStartupFailed, Profile: os.Getenv("UC_PROFILE"), Details: details,
		Retryable: status.Progress.AllowedActions.Retry || (status.Progress.Failure != nil && status.Progress.Failure.Retryable)}
}

// IncompatibleError renders an Incompatible outcome with the newer-daemon guard.
func IncompatibleError(o Outcome) *Error {
	return &Error{Kind: ErrIncompatible, Details: o.Details, ObservedVersion: o.ObservedVersion,
		Newer: o.ObservedVersion != nil && strictlyNewer(*o.ObservedVersion, buildinfo.PackageVersion)}
}

// strictlyNewer compares semver versions (pre-release aware), returning false
// when either side does not parse, like `running_daemon_is_strictly_newer`.
func strictlyNewer(observed, expected string) bool {
	a, okA := parseSemver(strings.TrimSpace(observed))
	b, okB := parseSemver(strings.TrimSpace(expected))
	return okA && okB && compareSemver(a, b) > 0
}

type semver struct {
	core [3]uint64
	pre  []string
}

func parseSemver(v string) (semver, bool) {
	if i := strings.IndexByte(v, '+'); i >= 0 {
		v = v[:i]
	}
	var s semver
	core, pre, hasPre := strings.Cut(v, "-")
	parts := strings.Split(core, ".")
	if len(parts) != 3 {
		return s, false
	}
	for i, p := range parts {
		if p == "" || (len(p) > 1 && p[0] == '0') {
			return s, false
		}
		n, err := strconv.ParseUint(p, 10, 64)
		if err != nil {
			return s, false
		}
		s.core[i] = n
	}
	if hasPre {
		if pre == "" {
			return s, false
		}
		s.pre = strings.Split(pre, ".")
	}
	return s, true
}

func compareSemver(a, b semver) int {
	for i := 0; i < 3; i++ {
		if a.core[i] != b.core[i] {
			if a.core[i] > b.core[i] {
				return 1
			}
			return -1
		}
	}
	switch {
	case len(a.pre) == 0 && len(b.pre) == 0:
		return 0
	case len(a.pre) == 0:
		return 1
	case len(b.pre) == 0:
		return -1
	}
	for i := 0; i < len(a.pre) && i < len(b.pre); i++ {
		x, y := a.pre[i], b.pre[i]
		xn, xErr := strconv.ParseUint(x, 10, 64)
		yn, yErr := strconv.ParseUint(y, 10, 64)
		switch {
		case xErr == nil && yErr == nil:
			if xn != yn {
				if xn > yn {
					return 1
				}
				return -1
			}
		case xErr == nil:
			return -1
		case yErr == nil:
			return 1
		case x != y:
			if x > y {
				return 1
			}
			return -1
		}
	}
	return len(a.pre) - len(b.pre)
}

// WaitHealthy polls until a Compatible daemon (optionally with the expected
// residency) answers, an Incompatible one appears, or the timeout passes.
func WaitHealthy(timeout time.Duration, residency string) error {
	return WaitHealthyForAttempt(timeout, residency, nil)
}

// WaitHealthyForAttempt never attributes another run's startup failure to a child.
func WaitHealthyForAttempt(timeout time.Duration, residency string, attempt *daemonproc.StartupAttempt) error {
	deadline := time.Now().Add(timeout)
	for {
		outcome, err := Probe()
		if err != nil {
			return err
		}
		switch outcome.Kind {
		case Compatible:
			if residency == "" || outcome.Health.Residency == residency {
				return nil
			}
		case Incompatible:
			return IncompatibleError(outcome)
		}
		if err := startupFailure(attempt); err != nil {
			return err
		}
		if !time.Now().Before(deadline) {
			return startupTimeout(timeout)
		}
		time.Sleep(pollInterval)
	}
}

// WaitForRunningDaemon waits for a foreground child to publish its endpoint.
func WaitForRunningDaemon() error { return WaitHealthy(StartupTimeout, "") }

func WaitAbsent(timeout time.Duration) error {
	deadline := time.Now().Add(timeout)
	for {
		outcome, err := Probe()
		if err != nil {
			return err
		}
		if outcome.Kind == Absent {
			live, err := daemonproc.ProfileHasLiveDaemon()
			if err != nil {
				return err
			}
			if !live {
				return nil
			}
		}
		if !time.Now().Before(deadline) {
			return &Error{Kind: ErrPromoteDrainTimeout, Timeout: timeout, Profile: os.Getenv("UC_PROFILE"), BaseURL: currentBaseURL()}
		}
		time.Sleep(pollInterval)
	}
}

// PromoteOneshot replaces a running oneshot daemon with a persistent one of the given residency: it asks the
// daemon to restart, waits for it to withdraw, spawns the replacement and waits until that is healthy. origin
// names the spawner ("cli", "gui") for the daemon's spawn-origin record.
func PromoteOneshot(target, origin string) error {
	client, err := daemonclient.FromEnv()
	if err != nil {
		return &Error{Kind: ErrPromoteRestart, Err: err}
	}
	err = client.Enveloped(context.Background(), daemonclient.Request{
		Method: http.MethodPost, Path: "/lifecycle/restart", JSON: map[string]string{"targetMode": target},
	}, nil)
	if err != nil {
		// A oneshot daemon that is winding down stops answering between the probe that found it and this request.
		// If it is gone by now there is nothing to restart and the replacement can be spawned.
		if outcome, probeErr := Probe(); probeErr != nil || outcome.Kind != Absent {
			return &Error{Kind: ErrPromoteRestart, Err: err}
		}
	}
	if err := WaitAbsent(PromoteDrainTimeout); err != nil {
		return err
	}
	return SpawnAndWait(origin, StartupTimeout, target)
}

// SpawnAndWait binds failure observation to the exact child run for every shell.
func SpawnAndWait(origin string, timeout time.Duration, residency string) error {
	attempt, err := daemonproc.SpawnDetachedDaemonForStartup(origin)
	if err != nil {
		return &Error{Kind: ErrSpawn, Err: err}
	}
	err = WaitHealthyForAttempt(timeout, residency, attempt)
	var failure *Error
	if errors.As(err, &failure) && failure.Kind == ErrStartupFailed {
		ctx, cancel := context.WithTimeout(context.Background(), predecessorRelease)
		releaseErr := daemonproc.ReleaseFailedStartup(ctx, attempt)
		cancel()
		if releaseErr != nil {
			return &Error{Kind: ErrStartupFailed, Profile: failure.Profile, Retryable: failure.Retryable,
				Details: fmt.Sprintf("%s; failed to release owned startup: %v", failure.Details, releaseErr)}
		}
	}
	return err
}

// RetryableStartupError permits an explicit user retry, never automatic respawn.
func RetryableStartupError(err error) bool {
	var failure *Error
	return errors.As(err, &failure) && (failure.Kind == ErrStartupTimeout || (failure.Kind == ErrStartupFailed && failure.Retryable))
}

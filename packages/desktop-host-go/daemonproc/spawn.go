package daemonproc

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"time"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/errctx"
)

// Environment contract shared with `uniclipd` (`uc-daemon-process::spawn_contract`).
const (
	NoTakeoverEnv  = "UC_DAEMON_NO_TAKEOVER"
	RunModeEnv     = "UC_DAEMON_RUN_MODE"
	RunModeServer  = "server"
	RunModeOneshot = "oneshot"
	spawnOriginEnv = "UC_DAEMON_SPAWN_ORIGIN"
	handoverFile   = ".uniclipd-handover.json"
)

// SpawnError mirrors `SpawnDaemonError`'s two variants and messages.
type SpawnError struct {
	ResolveBinary bool
	Err           error
}

func (e *SpawnError) Error() string {
	if e.ResolveBinary {
		return fmt.Sprintf("failed to resolve `uniclipd` binary for spawn: %v", e.Err)
	}
	return fmt.Sprintf("failed to spawn daemon process: %v", e.Err)
}

func (e *SpawnError) Unwrap() error { return e.Err }

func daemonBinaryName() string {
	if runtime.GOOS == "windows" {
		return "uniclipd.exe"
	}
	return "uniclipd"
}

// ResolveDaemonExe finds `uniclipd` next to this executable, then on PATH.
func ResolveDaemonExe() (string, error) {
	name := daemonBinaryName()
	if self, err := os.Executable(); err == nil {
		candidate := filepath.Join(filepath.Dir(self), name)
		if info, err := os.Stat(candidate); err == nil && info.Mode().IsRegular() {
			return candidate, nil
		}
	}
	path, err := exec.LookPath(name)
	if err != nil {
		return "", &SpawnError{ResolveBinary: true, Err: errctx.Wrap(fmt.Sprintf("`%s` not found as sibling of the spawning binary or in PATH", name), err)}
	}
	return path, nil
}

type handoverRecord struct {
	TargetMode string `json:"target_mode"`
	Generation uint64 `json:"generation"`
}

// SpawnDetachedDaemon starts `uniclipd` in its own session with null stdio,
// inheriting this process's environment, and applies a pending handover's
// run mode exactly like the Rust spawn contract.
func SpawnDetachedDaemon(origin string) error {
	_, err := SpawnDetachedDaemonForStartup(origin)
	return err
}

// StartupAttempt retains the actual child lifetime, not just its reusable PID.
// Only this spawn can construct the handle used to release a failed startup.
type StartupAttempt struct {
	pid        uint32
	instanceID string
	done       <-chan struct{}
}

// PID is the actual spawned child PID.
func (a *StartupAttempt) PID() uint32 { return a.pid }

// InstanceID is the random identity assigned before spawning this child.
func (a *StartupAttempt) InstanceID() string { return a.instanceID }

// Exited reports whether this exact child was reaped.
func (a *StartupAttempt) Exited() bool {
	select {
	case <-a.done:
		return true
	default:
		return false
	}
}

// Wait waits for this exact child without sending signals to any process.
func (a *StartupAttempt) Wait(ctx context.Context) error {
	select {
	case <-a.done:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}

// SpawnDetachedDaemonForStartup returns a handle to the exact spawned run.
// NoTakeover closes the race with another client that claims the profile first.
func SpawnDetachedDaemonForStartup(origin string) (*StartupAttempt, error) {
	exe, err := ResolveDaemonExe()
	if err != nil {
		return nil, err
	}
	nonce := make([]byte, 16)
	if _, err := rand.Read(nonce); err != nil {
		return nil, err
	}
	instanceID := hex.EncodeToString(nonce)
	cmd := exec.Command(exe)
	cmd.Env = append(os.Environ(), spawnOriginEnv+"="+origin, NoTakeoverEnv+"=1", "UC_DAEMON_STARTUP_ID="+instanceID)
	if root, err := AppDataRoot(); err == nil {
		if data, err := os.ReadFile(filepath.Join(root, handoverFile)); err == nil {
			var record handoverRecord
			if json.Unmarshal(data, &record) == nil {
				cmd.Env = append(cmd.Env, RunModeEnv+"="+record.TargetMode)
			}
		}
	}
	configureDetached(cmd)
	if err := cmd.Start(); err != nil {
		return nil, &SpawnError{Err: errctx.Wrap(fmt.Sprintf("failed to spawn daemon via `%s`", exe), err)}
	}
	// Reap the child when it exits. `Process.Release` would leave it a zombie of this process until we exit,
	// and a zombie still passes the `kill(pid, 0)` liveness check, so a stopped daemon would look alive.
	done := make(chan struct{})
	go func() { _ = cmd.Wait(); close(done) }()
	return &StartupAttempt{pid: uint32(cmd.Process.Pid), instanceID: instanceID, done: done}, nil
}

// ReleaseFailedStartup requests an orderly exit only from this host's child.
// The startup service itself checks terminal state and run identity.
func ReleaseFailedStartup(ctx context.Context, attempt *StartupAttempt) error {
	if attempt != nil && attempt.done == nil {
		return fmt.Errorf("startup attempt has no owned child handle")
	}
	if attempt == nil || attempt.Exited() {
		return nil
	}
	conn, err := ReadStartupConnFile()
	if err != nil {
		return err
	}
	if conn == nil || conn.PID != attempt.PID() || conn.InstanceID != attempt.InstanceID() {
		return fmt.Errorf("the owned startup service has not published its run identity; retry later")
	}
	status, err := requestOwnedStartup(ctx, conn, attempt, "/startup/exit")
	if err != nil {
		return err
	}
	if status != http.StatusAccepted {
		return fmt.Errorf("owned daemon refused startup retry (HTTP %d)", status)
	}
	return attempt.Wait(ctx)
}

// StopStartingDaemon requests shutdown from the exact spawned run, even before daemon.conn exists.
func StopStartingDaemon(ctx context.Context, attempt *StartupAttempt) error {
	if attempt == nil || attempt.done == nil {
		return fmt.Errorf("startup attempt has no owned child handle")
	}
	ticker := time.NewTicker(100 * time.Millisecond)
	defer ticker.Stop()
	for {
		if attempt.Exited() {
			return nil
		}
		conn, err := ReadStartupConnFile()
		if err != nil {
			return err
		}
		if conn != nil && conn.PID == attempt.PID() && conn.InstanceID == attempt.InstanceID() {
			status, err := requestOwnedStartup(ctx, conn, attempt, "/startup/stop")
			if err == nil {
				if status != http.StatusAccepted {
					return fmt.Errorf("owned daemon refused startup stop (HTTP %d)", status)
				}
				return attempt.Wait(ctx)
			}
			if ctx.Err() != nil {
				return ctx.Err()
			}
		}
		select {
		case <-attempt.done:
			return nil
		case <-ctx.Done():
			return ctx.Err()
		case <-ticker.C:
		}
	}
}

func requestOwnedStartup(ctx context.Context, conn *ConnFile, attempt *StartupAttempt, path string) (int, error) {
	request, err := http.NewRequestWithContext(ctx, http.MethodPost, conn.BaseURL()+path, nil)
	if err != nil {
		return 0, err
	}
	request.Header.Set("Authorization", "Bearer "+conn.Token)
	request.Header.Set("X-UniClipboard-Startup-Instance", attempt.InstanceID())
	client := &http.Client{Timeout: 3 * time.Second, Transport: &http.Transport{Proxy: nil}, CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}
	defer client.CloseIdleConnections()
	response, err := client.Do(request)
	if err != nil {
		return 0, err
	}
	response.Body.Close()
	return response.StatusCode, nil
}

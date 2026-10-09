package daemonproc

import (
	"encoding/json"
	"fmt"
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
	waitRecordedDaemonExit(previousDaemonExitWait)
	exe, err := ResolveDaemonExe()
	if err != nil {
		return err
	}
	cmd := exec.Command(exe)
	cmd.Env = append(os.Environ(), spawnOriginEnv+"="+origin)
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
		return &SpawnError{Err: errctx.Wrap(fmt.Sprintf("failed to spawn daemon via `%s`", exe), err)}
	}
	return cmd.Process.Release()
}

// previousDaemonExitWait bounds how long a spawn waits for a daemon that has already withdrawn from the network to
// leave the process table.
const previousDaemonExitWait = 10 * time.Second

// waitRecordedDaemonExit waits for a draining daemon to exit. A daemon removes `daemon.conn` and stops answering
// `/health` while it is still shutting down, but it holds the instance lock until the process ends; a daemon spawned
// in that window fails with "failed to acquire daemon instance lock" (os error 33 on Windows).
//
// It waits only when `.daemon-pid` names a process that is verifiably that daemon (see recordedProcessIsDaemon) and
// `daemon.conn` no longer advertises it. A published daemon is left to the caller's reuse logic, a stale or reused
// pid is not waited for, and nothing is ever terminated here.
func waitRecordedDaemonExit(timeout time.Duration) {
	deadline := time.Now().Add(timeout)
	for time.Now().Before(deadline) {
		meta, err := ReadPidMetadata()
		if err != nil || meta == nil || meta.Mode == "in_process" || !recordedProcessIsDaemon(*meta) {
			return
		}
		if conn, err := ReadConnFile(); err != nil || (conn != nil && conn.PID == meta.PID) {
			return
		}
		time.Sleep(100 * time.Millisecond)
	}
}

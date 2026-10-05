package daemonproc

import (
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
)

// Environment contract shared with `uniclipd` (`uc-daemon-process::spawn_contract`).
const (
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
		return "", &SpawnError{ResolveBinary: true, Err: fmt.Errorf("`%s` not found as sibling of the spawning binary or in PATH: %w", name, err)}
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
func SpawnDetachedDaemon() error {
	exe, err := ResolveDaemonExe()
	if err != nil {
		return err
	}
	cmd := exec.Command(exe)
	cmd.Env = append(os.Environ(), spawnOriginEnv+"=cli")
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
		return &SpawnError{Err: fmt.Errorf("failed to spawn daemon via `%s`: %w", exe, err)}
	}
	return cmd.Process.Release()
}

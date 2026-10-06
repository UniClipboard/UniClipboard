package daemonproc

import (
	"encoding/json"
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"strconv"
	"strings"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/errctx"
)

const pidFileName = ".daemon-pid"

// PidMetadata is the daemon's `.daemon-pid` record.
type PidMetadata struct {
	PID            uint32 `json:"pid"`
	Mode           string `json:"mode"`
	StartedAtMs    uint64 `json:"startedAtMs"`
	SpawnedBy      string `json:"spawnedBy"`
	PackageVersion string `json:"packageVersion"`
}

// ReadPidMetadata reads `.daemon-pid`, accepting the legacy bare-integer form.
func ReadPidMetadata() (*PidMetadata, error) {
	root, err := AppDataRoot()
	if err != nil {
		return nil, fmt.Errorf("failed to resolve application directories: %w", err)
	}
	path := filepath.Join(root, pidFileName)
	raw, err := os.ReadFile(path)
	if errors.Is(err, fs.ErrNotExist) {
		return nil, nil
	}
	if err != nil {
		return nil, errctx.Wrap(fmt.Sprintf("failed to read daemon pid file %s", path), err)
	}
	trimmed := strings.TrimSpace(string(raw))
	if trimmed == "" {
		return nil, nil
	}
	var meta PidMetadata
	if err := json.Unmarshal([]byte(trimmed), &meta); err == nil && meta.Mode != "" {
		return &meta, nil
	}
	pid, err := strconv.ParseUint(trimmed, 10, 32)
	if err != nil {
		return nil, fmt.Errorf("failed to parse daemon pid file %s as JSON metadata or u32", path)
	}
	return &PidMetadata{PID: uint32(pid), Mode: "standalone", SpawnedBy: "unknown"}, nil
}

// IsActiveDaemon mirrors `verify_pid_identity`: the pid is alive and, where
// the platform exposes it, its executable is a daemon binary.
func IsActiveDaemon(pid uint32) bool {
	if !isPidAlive(pid) {
		return false
	}
	if exe, ok := processExe(pid); ok {
		name := exe
		if i := strings.LastIndexAny(exe, `/\`); i >= 0 {
			name = exe[i+1:]
		}
		return isDaemonBinaryName(name)
	}
	return true
}

func isDaemonBinaryName(name string) bool {
	base := strings.TrimSuffix(name, ".exe")
	return base == "uniclipd" || strings.HasPrefix(base, "uniclipd-") ||
		base == "uniclip" || strings.HasPrefix(base, "uniclip-")
}

// ConnPointsToLiveDaemon reports whether `daemon.conn` names a live daemon.
func ConnPointsToLiveDaemon() (bool, error) {
	conn, err := ReadConnFile()
	if err != nil {
		return false, err
	}
	if conn == nil {
		return false, nil
	}
	return IsActiveDaemon(conn.PID), nil
}

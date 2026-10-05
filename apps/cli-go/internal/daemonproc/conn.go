// Package daemonproc mirrors `uc-daemon-process`: the daemon connection file,
// pid metadata, process identity, detached spawn, and handover record.
package daemonproc

import (
	"encoding/json"
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/apppaths"
)

const (
	connFormat   = 1
	connFileName = "daemon.conn"
)

// ConnFile is the daemon-published connection record (`daemon.conn`).
type ConnFile struct {
	Format      uint32 `json:"format"`
	Host        string `json:"host"`
	Port        uint16 `json:"port"`
	Token       string `json:"token"`
	PID         uint32 `json:"pid"`
	StartedAtMs uint64 `json:"startedAtMs"`
}

// BaseURL is the HTTP base URL the connection file advertises.
func (c ConnFile) BaseURL() string { return fmt.Sprintf("http://%s:%d", c.Host, c.Port) }

// AppDataRoot returns the profile data root or the Rust-equivalent error.
func AppDataRoot() (string, error) {
	root, ok := apppaths.AppDataRoot()
	if !ok {
		return "", errors.New("the system data-local directory is unavailable")
	}
	return root, nil
}

// ConnPath resolves the profile's `daemon.conn` path.
func ConnPath() (string, error) {
	root, err := AppDataRoot()
	if err != nil {
		return "", err
	}
	return filepath.Join(root, connFileName), nil
}

// ReadConnFile reads `daemon.conn`; a missing file is (nil, nil).
func ReadConnFile() (*ConnFile, error) {
	path, err := ConnPath()
	if err != nil {
		return nil, err
	}
	data, err := os.ReadFile(path)
	if errors.Is(err, fs.ErrNotExist) {
		return nil, nil
	}
	if err != nil {
		return nil, fmt.Errorf("failed to read daemon connection file at %s: %w", path, err)
	}
	var conn ConnFile
	if err := json.Unmarshal(data, &conn); err != nil {
		return nil, fmt.Errorf("failed to parse daemon connection file at %s: %w", path, err)
	}
	if conn.Format != connFormat {
		return nil, fmt.Errorf("unsupported daemon connection file format %d at %s (expected %d)", conn.Format, path, connFormat)
	}
	return &conn, nil
}

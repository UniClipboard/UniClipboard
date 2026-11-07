// Package paths locates uni's files. Everything lives in one directory so a
// second instance (tests, a different profile) can be run with UNI_HOME.
package paths

import (
	"os"
	"path/filepath"
)

// Dir is the state directory: database, socket, received files.
func Dir() (string, error) {
	if d := os.Getenv("UNI_HOME"); d != "" {
		return d, os.MkdirAll(d, 0o700)
	}
	base, err := os.UserConfigDir()
	if err != nil {
		return "", err
	}
	d := filepath.Join(base, "uni")
	return d, os.MkdirAll(d, 0o700)
}

func sub(name string) (string, error) {
	d, err := Dir()
	if err != nil {
		return "", err
	}
	return filepath.Join(d, name), nil
}

// Socket is the daemon's control socket, the way /var/run/docker.sock is
// docker's. AF_UNIX works on Windows 10+ too, so no named pipes are needed.
// Unix socket paths are short (104 bytes on macOS), so the default sits in the
// state directory and UNI_HOME users should keep it short.
func Socket() (string, error) {
	if s := os.Getenv("UNI_SOCK"); s != "" {
		return s, nil
	}
	return sub("uni.sock")
}

func Database() (string, error) { return sub("uni.db") }
func Inbox() (string, error)    { return sub("inbox") }

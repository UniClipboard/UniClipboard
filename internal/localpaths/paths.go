// Package paths locates unic's files. Everything lives in one directory so a
// second instance (tests, a different profile) can be run with UNIC_HOME.
package paths

import (
	"errors"
	"os"
	"path/filepath"
	"runtime"
)

// V1DataDir locates the original default profile without creating directories.
func V1DataDir() (string, error) {
	home, err := os.UserHomeDir()
	if err != nil {
		return "", err
	}
	var base string
	switch runtime.GOOS {
	case "darwin":
		base = filepath.Join(home, "Library", "Application Support")
	case "windows":
		base = os.Getenv("LOCALAPPDATA")
	default:
		base = os.Getenv("XDG_DATA_HOME")
		if base == "" {
			base = filepath.Join(home, ".local", "share")
		}
	}
	if base == "" {
		return "", errors.New("cannot locate v1 data directory")
	}
	return filepath.Join(base, "app.uniclipboard.desktop"), nil
}

// Dir is the state directory: database, socket, received files.
func Dir() (string, error) {
	if d := os.Getenv("UNIC_HOME"); d != "" {
		if err := os.MkdirAll(d, 0700); err != nil {
			return "", err
		}
		return filepath.EvalSymlinks(d)
	}
	base, err := os.UserConfigDir()
	if err != nil {
		return "", err
	}
	d := filepath.Join(base, "unic")
	if err := os.MkdirAll(d, 0700); err != nil {
		return "", err
	}
	return filepath.EvalSymlinks(d)
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
// state directory and UNIC_HOME users should keep it short.
func Socket() (string, error) {
	if s := os.Getenv("UNIC_SOCK"); s != "" {
		return s, nil
	}
	return sub("unic.sock")
}

func Database() (string, error) { return sub("unic.db") }
func Inbox() (string, error)    { return sub("inbox") }

// LuaDir is where installed Lua plugins live, one directory each.
func LuaDir() (string, error) { return sub(filepath.Join("plugins", "lua")) }

// Config is the TOML configuration file.
func Config() (string, error) { return sub("config.toml") }

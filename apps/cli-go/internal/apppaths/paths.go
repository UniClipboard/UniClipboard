// Package apppaths mirrors the Rust `uc-app-paths` policy: the data root,
// profile suffix, portable mode, and log directory. It is the single place
// the Go CLI derives profile-scoped filesystem locations.
package apppaths

import (
	"os"
	"path/filepath"
	"strings"
	"sync"
)

// AppDirName is the application directory name shared with the GUI and daemon.
const AppDirName = "app.uniclipboard.desktop"

const (
	portableMarker     = "portable.dat"
	portableDataSubdir = "data"
)

// Profile returns the active profile from UC_PROFILE; an empty value means
// the default profile.
func Profile() (string, bool) {
	profile := os.Getenv("UC_PROFILE")
	return profile, profile != ""
}

// AppDirNameForProfile returns the profile-suffixed application directory name.
func AppDirNameForProfile() string {
	if profile, ok := Profile(); ok {
		return AppDirName + "-" + profile
	}
	return AppDirName
}

var portableRoot = sync.OnceValue(func() string {
	forced := false
	switch v := strings.TrimSpace(os.Getenv("UC_PORTABLE")); {
	case v == "1", strings.EqualFold(v, "true"), strings.EqualFold(v, "yes"):
		forced = true
	}
	exe, err := os.Executable()
	if err != nil {
		return ""
	}
	dir := filepath.Dir(exe)
	if forced {
		return filepath.Join(dir, portableDataSubdir)
	}
	if info, err := os.Stat(filepath.Join(dir, portableMarker)); err == nil && info.Mode().IsRegular() {
		return filepath.Join(dir, portableDataSubdir)
	}
	return ""
})

// IsPortable reports whether the CLI runs as a portable installation.
func IsPortable() bool { return portableRoot() != "" }

func baseDataLocalDir() (string, bool) {
	if root := portableRoot(); root != "" {
		return root, true
	}
	return dataLocalDir()
}

func baseCacheDir() (string, bool) {
	if root := portableRoot(); root != "" {
		return root, true
	}
	return cacheDir()
}

// AppDataRoot is the profile-scoped data root (`daemon.conn`, `.daemon-pid`, ...).
func AppDataRoot() (string, bool) {
	base, ok := baseDataLocalDir()
	if !ok {
		return "", false
	}
	return filepath.Join(base, AppDirNameForProfile()), true
}

// AppCacheRoot is the profile-scoped cache root.
func AppCacheRoot() (string, bool) {
	base, ok := baseCacheDir()
	if !ok {
		return "", false
	}
	return filepath.Join(base, AppDirNameForProfile()), true
}

// AppLogDir mirrors `uc_app_paths::app_log_dir`.
func AppLogDir() (string, bool) {
	profile, hasProfile := Profile()
	if hasProfile && !isSafeProfileComponent(profile) {
		return "", false
	}
	if root := portableRoot(); root != "" {
		return filepath.Join(root, "logs"), true
	}
	name := AppDirName
	if hasProfile {
		name = AppDirName + "-" + profile
	}
	return platformLogDir(name)
}

func isSafeProfileComponent(profile string) bool {
	if profile == "" {
		return false
	}
	for i := 0; i < len(profile); i++ {
		c := profile[i]
		ok := c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' || c == '-' || c == '_'
		if !ok {
			return false
		}
	}
	return true
}

// homeDir follows the Rust `dirs` crate: $HOME on Unix.
func homeDir() (string, bool) {
	if home := os.Getenv("HOME"); home != "" {
		return home, true
	}
	home, err := os.UserHomeDir()
	return home, err == nil && home != ""
}

// xdgDir returns an absolute XDG override or the home-relative fallback,
// matching `dirs-sys` (relative XDG values are ignored).
func xdgDir(env string, fallback ...string) (string, bool) {
	if v := os.Getenv(env); v != "" && filepath.IsAbs(v) {
		return v, true
	}
	home, ok := homeDir()
	if !ok {
		return "", false
	}
	return filepath.Join(append([]string{home}, fallback...)...), true
}

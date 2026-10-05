//go:build linux || freebsd || openbsd || netbsd || dragonfly

package apppaths

import "path/filepath"

func dataLocalDir() (string, bool) { return xdgDir("XDG_DATA_HOME", ".local", "share") }

func cacheDir() (string, bool) { return xdgDir("XDG_CACHE_HOME", ".cache") }

func platformLogDir(appDirName string) (string, bool) {
	base, ok := xdgDir("XDG_STATE_HOME", ".local", "state")
	if !ok {
		if base, ok = dataLocalDir(); !ok {
			return "", false
		}
	}
	return filepath.Join(base, appDirName, "logs"), true
}

//go:build linux

package main

import (
	"os"
	"os/exec"
	"strings"
)

// AppRun (and the linuxdeploy GTK hook it sources) point the process at the AppImage's own libraries and data:
// LD_LIBRARY_PATH, XDG_DATA_DIRS, GIO_MODULE_DIR, GTK_*, GSETTINGS_SCHEMA_DIR, ... Host programs the GUI starts for the user (xdg-open and the
// handlers it launches) are not part of the bundle and must resolve their own libraries and data, so they get the environment without those entries
// (docs/architecture/gui-go-linux-appimage-host-helpers.md). The GUI itself, the daemon and the relaunch of the AppImage keep the full environment.

// xdgDataDirsHookPrefix is what the GTK hook prepends: `$APPDIR/usr/share:/usr/share:` followed by the caller's original value.
func xdgDataDirsHookPrefix(appDir string) string { return appDir + "/usr/share:/usr/share:" }

// hostHelperEnvironment returns environ without the AppImage entries. Outside an AppImage (no APPDIR) it returns environ unchanged.
func hostHelperEnvironment(environ []string, appDir string) []string {
	if appDir == "" {
		return environ
	}
	underAppDir := func(p string) bool { return p == appDir || strings.HasPrefix(p, appDir+"/") }
	out := make([]string, 0, len(environ))
	for _, kv := range environ {
		key, value, ok := strings.Cut(kv, "=")
		if !ok {
			out = append(out, kv)
			continue
		}
		switch {
		case key == "XDG_DATA_DIRS":
			// The hook appends the original value after its own prefix: strip that prefix to get the original back (an empty original means unset).
			original := strings.TrimPrefix(value, xdgDataDirsHookPrefix(appDir))
			if original == value {
				original = dropAppDirEntries(value, underAppDir)
			}
			if original != "" {
				out = append(out, key+"="+original)
			}
		case key == "LD_LIBRARY_PATH":
			if kept := dropAppDirEntries(value, underAppDir); kept != "" {
				out = append(out, key+"="+kept)
			}
		case key == "PWD" || underAppDir(value) && !strings.Contains(value, ":"):
			// APPDIR itself, GIO_MODULE_DIR, GTK_PATH, GSETTINGS_SCHEMA_DIR, ...: single paths into the mount. PWD is the mount too (AppRun runs from $APPDIR/usr);
			// the caller sets the working directory and PWD of the helper.
		default:
			out = append(out, kv)
		}
	}
	return out
}

func dropAppDirEntries(list string, underAppDir func(string) bool) string {
	var kept []string
	for _, entry := range strings.Split(list, ":") {
		if entry != "" && !underAppDir(entry) {
			kept = append(kept, entry)
		}
	}
	return strings.Join(kept, ":")
}

// startHostHelper starts a host program detached from the GUI's AppImage environment. Its working directory is the user's home, not the mount.
func startHostHelper(name string, args ...string) error {
	cmd := exec.Command(name, args...)
	if appDir := os.Getenv("APPDIR"); appDir != "" {
		cmd.Env = hostHelperEnvironment(os.Environ(), appDir)
		if home, err := os.UserHomeDir(); err == nil {
			cmd.Dir = home
			cmd.Env = append(cmd.Env, "PWD="+home)
		}
	}
	if err := cmd.Start(); err != nil {
		return err
	}
	go func() { _ = cmd.Wait() }()
	return nil
}

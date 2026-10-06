// Package apppaths mirrors the Rust `uc-app-paths` policy: the data root,
// profile suffix, portable mode, and log directory. It is the single place
// the Go hosts derive profile-scoped filesystem locations.
package apppaths

import (
	"errors"
	"fmt"
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

// portableResolution is the outcome of the portable-mode decision (mirrors the Rust `PortableResolution`).
type portableResolution struct {
	root string // portable data root; empty when not portable
	err  error  // portable was requested but cannot be honoured: no resolver falls back to the system directories
}

// AppImagePortableHomeSuffix is the suffix of the directory next to an AppImage file that makes it portable: the
// AppImage runtime's own convention (`<AppImage>.home`), which it also uses to redirect $HOME.
const AppImagePortableHomeSuffix = ".home"

var portableState = sync.OnceValue(func() portableResolution {
	forced := false
	switch v := strings.TrimSpace(os.Getenv("UC_PORTABLE")); {
	case v == "1", strings.EqualFold(v, "true"), strings.EqualFold(v, "yes"):
		forced = true
	}
	exe, err := os.Executable()
	if err != nil {
		return portableResolution{}
	}
	return resolvePortable(exe, forced, os.Getenv("APPDIR"), os.Getenv("APPIMAGE"))
})

// resolvePortable decides portable mode from explicit inputs. Inside an AppImage (the executable lives under
// APPDIR) the executable directory is a read-only mount, so the AppImage file's `.home` directory is the portable
// root; anywhere else the executable directory and the `portable.dat` marker apply.
func resolvePortable(exe string, forced bool, appDir, appImage string) portableResolution {
	if insideAppDir(exe, appDir) {
		return resolveAppImagePortable(forced, appImage)
	}
	dir := filepath.Dir(exe)
	if forced {
		return portableResolution{root: filepath.Join(dir, portableDataSubdir)}
	}
	if info, err := os.Stat(filepath.Join(dir, portableMarker)); err == nil && info.Mode().IsRegular() {
		return portableResolution{root: filepath.Join(dir, portableDataSubdir)}
	}
	return portableResolution{}
}

// insideAppDir reports whether exe is under the AppImage mount APPDIR points to; an APPIMAGE variable inherited by an
// unrelated process does not count.
func insideAppDir(exe, appDir string) bool {
	if appDir == "" {
		return false
	}
	if resolved, err := filepath.EvalSymlinks(appDir); err == nil {
		appDir = resolved
	}
	if resolved, err := filepath.EvalSymlinks(exe); err == nil {
		exe = resolved
	}
	rel, err := filepath.Rel(appDir, exe)
	return err == nil && rel != ".." && !strings.HasPrefix(rel, ".."+string(os.PathSeparator)) && !filepath.IsAbs(rel)
}

func resolveAppImagePortable(forced bool, appImage string) portableResolution {
	valid := false
	if appImage != "" && filepath.IsAbs(appImage) {
		if info, err := os.Stat(appImage); err == nil && info.Mode().IsRegular() {
			valid = true
		}
	}
	if !valid {
		if forced {
			return portableResolution{err: errors.New("UC_PORTABLE is set, but this process runs inside an AppImage without a valid $APPIMAGE " +
				"(an absolute path to the AppImage file); start the .AppImage file itself")}
		}
		return portableResolution{}
	}
	home := appImage + AppImagePortableHomeSuffix
	if info, err := os.Stat(home); err == nil && info.IsDir() {
		return portableResolution{root: filepath.Join(home, portableDataSubdir)}
	}
	if forced {
		return portableResolution{err: fmt.Errorf("UC_PORTABLE is set, but the portable home directory %s does not exist; create it with "+
			"`%s --appimage-portable-home` before starting (the AppImage runtime only redirects $HOME to a directory that exists at startup)", home, appImage)}
	}
	return portableResolution{}
}

// IsPortable reports whether the CLI runs as a portable installation.
func IsPortable() bool { return portableState().root != "" }

// PortableError reports why portable mode was requested but cannot be honoured, or nil. While it is non-nil every
// directory resolver in this package reports "unavailable" instead of falling back to the system directories.
func PortableError() error { return portableState().err }

// PortableHome returns the AppImage portable home directory (`<AppImage>.home`) when portable mode is active inside an
// AppImage: the directory the runtime redirected $HOME to.
func PortableHome() (string, bool) {
	root := portableState().root
	if root == "" || filepath.Base(root) != portableDataSubdir {
		return "", false
	}
	home := filepath.Dir(root)
	return home, strings.HasSuffix(home, AppImagePortableHomeSuffix) && insideAppDir(currentExe(), os.Getenv("APPDIR"))
}

func currentExe() string {
	exe, _ := os.Executable()
	return exe
}

func baseDataLocalDir() (string, bool) {
	if state := portableState(); state.err != nil {
		return "", false
	} else if state.root != "" {
		return state.root, true
	}
	return dataLocalDir()
}

func baseCacheDir() (string, bool) {
	if state := portableState(); state.err != nil {
		return "", false
	} else if state.root != "" {
		return state.root, true
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
	if state := portableState(); state.err != nil {
		return "", false
	} else if state.root != "" {
		return filepath.Join(state.root, "logs"), true
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

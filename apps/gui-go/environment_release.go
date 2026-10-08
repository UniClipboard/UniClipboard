//go:build release

package main

import (
	"fmt"
	"os"
	"path/filepath"
	"runtime"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/apppaths"
)

// releaseBuild marks the shipped product form: the primary profile of the installed (or portable) application, no
// development profile, no test sandbox. It is a separate build tag from `production` (which only means "no
// devtools" and is also used by the macOS manual build, a development build).
const releaseBuild = true

// validateRelease guards the shipped form. The product is single-profile (the Tauri shell made the same decision),
// so every development and test knob is refused rather than silently honoured: a profile, a daemon override, the
// development environment marker and the isolated test mode. Portable mode is allowed (`portable.dat` next to the
// executable, or UC_PORTABLE) on Windows and Linux; macOS ships only as the signed .app bundle, whose seal a marker
// file inside it would break, so portable mode is refused there. Windows, Linux and macOS ship in this form; on any
// other platform the tag is refused, so a release-tagged binary can never be launched against a real data root by accident.
func validateRelease() error {
	if runtime.GOOS != "windows" && runtime.GOOS != "linux" && runtime.GOOS != "darwin" {
		return fmt.Errorf("the release form is only available on Windows, Linux and macOS")
	}
	for _, key := range []string{"UC_PROFILE", "UC_GUI_GO_ISOLATED", "UNICLIPBOARD_DAEMON_BASE_URL", "UNICLIPBOARD_DAEMON_TOKEN_PATH", "UC_DAEMON_RUN_MODE"} {
		if os.Getenv(key) != "" {
			return fmt.Errorf("the release build refuses the development setting %s", key)
		}
	}
	if os.Getenv("UNICLIPBOARD_ENV") == "development" {
		return fmt.Errorf("the release build refuses UNICLIPBOARD_ENV=development")
	}
	if runtime.GOOS == "darwin" {
		if os.Getenv("UC_PORTABLE") != "" || apppaths.IsPortable() {
			return fmt.Errorf("the release build refuses portable mode on macOS: the data root is ~/Library/Application Support")
		}
		return nil
	}
	return validatePortableDataRoot()
}

// validatePortableDataRoot refuses to start a portable installation whose data root cannot be used. There is no
// fallback: using the per-user system directories instead would silently move a portable installation's data and
// secrets into the shared profile. Two failures are told apart so the message can say what to do: portable was
// requested but cannot be resolved (apppaths.PortableError, e.g. a missing AppImage `.home` directory), and the
// resolved root is not writable (a read-only location, or an owner other than the current user).
func validatePortableDataRoot() error {
	if err := apppaths.PortableError(); err != nil {
		return fmt.Errorf("portable mode cannot start: %w", err)
	}
	if !apppaths.IsPortable() {
		return nil
	}
	root, ok := apppaths.AppDataRoot()
	if !ok {
		return fmt.Errorf("portable mode cannot start: the data directory is unavailable")
	}
	if err := os.MkdirAll(root, 0o700); err != nil {
		return fmt.Errorf("portable mode cannot start: the data directory %s cannot be created: %w; make its parent writable for the current user", root, err)
	}
	probe, err := os.CreateTemp(root, ".portable-write-check-*")
	if err != nil {
		return fmt.Errorf("portable mode cannot start: the data directory %s is not writable: %w; make it writable for the current user", filepath.Clean(root), err)
	}
	name := probe.Name()
	_ = probe.Close()
	return os.Remove(name)
}

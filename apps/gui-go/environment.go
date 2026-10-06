package main

import (
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"runtime"
	"strings"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/apppaths"
)

var profilePattern = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$`)

// validateEnvironment refuses to start against anything but a development
// profile. Two modes exist:
//
//   - isolated (UC_GUI_GO_ISOLATED=1): a throwaway sandbox and a gui-go-* profile,
//     used by the end-to-end tests, so nothing outside that sandbox is touched
//     (see validateIsolation for what the sandbox is on each platform);
//   - development profile: the developer's own HOME with an explicit UC_PROFILE,
//     like `bun tauri:dev`; the profile keeps data, keychain entries and the
//     daemon separate from the production app.
//
// Both require UNICLIPBOARD_ENV=development; production data is never reachable. The `release` build tag is the one
// exception: it is the shipped product form and takes its own checks (environment_release.go).
func validateEnvironment() error {
	if releaseBuild {
		return validateRelease()
	}
	if runtime.GOOS != "darwin" && runtime.GOOS != "windows" {
		return fmt.Errorf("the Go GUI currently runs only on macOS and Windows")
	}
	if os.Getenv("UNICLIPBOARD_ENV") != "development" {
		return fmt.Errorf("the Go GUI is a development build: set UNICLIPBOARD_ENV=development")
	}
	if !profilePattern.MatchString(os.Getenv("UC_PROFILE")) {
		return fmt.Errorf("set UC_PROFILE to a development profile name (letters, digits, '_' or '-')")
	}
	for _, key := range []string{"UNICLIPBOARD_DAEMON_BASE_URL", "UNICLIPBOARD_DAEMON_TOKEN_PATH", "UC_DAEMON_RUN_MODE"} {
		if os.Getenv(key) != "" {
			return fmt.Errorf("the Go GUI refuses the override %s", key)
		}
	}
	if os.Getenv("UC_GUI_GO_ISOLATED") == "1" {
		return validateIsolation()
	}
	if os.Getenv("UC_PORTABLE") != "" || apppaths.IsPortable() {
		return fmt.Errorf("the Go GUI refuses portable mode")
	}
	return nil
}

// ensureInside reports an error unless root, or its nearest existing ancestor, resolves to base or below.
func ensureInside(root, base string) error {
	ancestor := root
	for {
		_, err := os.Stat(ancestor)
		if err == nil {
			break
		}
		if !os.IsNotExist(err) {
			return err
		}
		parent := filepath.Dir(ancestor)
		if parent == ancestor {
			return fmt.Errorf("data root has no existing ancestor")
		}
		ancestor = parent
	}
	ancestor, err := filepath.EvalSymlinks(ancestor)
	if err != nil {
		return err
	}
	if ancestor != base && !strings.HasPrefix(ancestor, base+string(os.PathSeparator)) {
		return fmt.Errorf("data root escapes the isolated sandbox")
	}
	return nil
}

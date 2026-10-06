package main

import (
	"fmt"
	"os"
	"os/user"
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
//   - isolated (UC_GUI_GO_ISOLATED=1): a throwaway HOME and a gui-go-* profile,
//     used by the end-to-end tests, so nothing outside that HOME is touched;
//   - development profile: the developer's own HOME with an explicit UC_PROFILE,
//     like `bun tauri:dev`; the profile keeps data, keychain entries and the
//     daemon separate from the production app.
//
// Both require UNICLIPBOARD_ENV=development; production data is never reachable.
func validateEnvironment() error {
	if runtime.GOOS != "darwin" {
		return fmt.Errorf("the Go GUI currently runs only on macOS")
	}
	if os.Getenv("UNICLIPBOARD_ENV") != "development" {
		return fmt.Errorf("the Go GUI is a development build: set UNICLIPBOARD_ENV=development")
	}
	if !profilePattern.MatchString(os.Getenv("UC_PROFILE")) {
		return fmt.Errorf("set UC_PROFILE to a development profile name (letters, digits, '_' or '-')")
	}
	for _, key := range []string{"UNICLIPBOARD_DAEMON_BASE_URL", "UNICLIPBOARD_DAEMON_TOKEN_PATH", "UC_PORTABLE", "UC_DAEMON_RUN_MODE"} {
		if os.Getenv(key) != "" {
			return fmt.Errorf("the Go GUI refuses the override %s", key)
		}
	}
	if apppaths.IsPortable() {
		return fmt.Errorf("the Go GUI refuses portable mode")
	}
	if os.Getenv("UC_GUI_GO_ISOLATED") == "1" {
		return validateIsolatedHome()
	}
	return nil
}

// validateIsolatedHome enforces the test mode: throwaway HOME, gui-go-* profile,
// no system clipboard, and a data root that stays inside that HOME.
func validateIsolatedHome() error {
	u, err := user.Current()
	if err != nil {
		return err
	}
	home, err := filepath.EvalSymlinks(os.Getenv("HOME"))
	if err != nil {
		return err
	}
	real, err := filepath.EvalSymlinks(u.HomeDir)
	if err != nil {
		return err
	}
	if !strings.HasPrefix(filepath.Base(home), "uc-gui-go-") || home == real ||
		strings.HasPrefix(home, real+string(os.PathSeparator)+"Library"+string(os.PathSeparator)) ||
		!strings.HasPrefix(os.Getenv("UC_PROFILE"), "gui-go-") || os.Getenv("UC_DISABLE_SYSTEM_CLIPBOARD") != "1" {
		return fmt.Errorf("isolated mode requires a uc-gui-go-* HOME, a gui-go-* profile and UC_DISABLE_SYSTEM_CLIPBOARD=1")
	}
	root, ok := apppaths.AppDataRoot()
	if !ok {
		return fmt.Errorf("data root unavailable")
	}
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
	ancestor, err = filepath.EvalSymlinks(ancestor)
	if err != nil {
		return err
	}
	if ancestor != home && !strings.HasPrefix(ancestor, home+string(os.PathSeparator)) {
		return fmt.Errorf("data root escapes the isolated HOME")
	}
	return nil
}

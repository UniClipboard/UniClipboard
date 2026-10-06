//go:build release

package main

import (
	"fmt"
	"os"
	"runtime"
)

// releaseBuild marks the shipped product form: the primary profile of the installed (or portable) application, no
// development profile, no test sandbox. It is a separate build tag from `production` (which only means "no
// devtools" and is also used by the macOS manual build, a development build).
const releaseBuild = true

// validateRelease guards the shipped form. The product is single-profile (the Tauri shell made the same decision),
// so every development and test knob is refused rather than silently honoured: a profile, a daemon override, the
// development environment marker and the isolated test mode. Portable mode is allowed (`portable.dat` next to the
// executable, or UC_PORTABLE). Only Windows ships in this form so far; on other platforms the tag is refused, so a
// release-tagged binary can never be launched against a real data root by accident.
func validateRelease() error {
	if runtime.GOOS != "windows" {
		return fmt.Errorf("the release form is only available on Windows so far")
	}
	for _, key := range []string{"UC_PROFILE", "UC_GUI_GO_ISOLATED", "UNICLIPBOARD_DAEMON_BASE_URL", "UNICLIPBOARD_DAEMON_TOKEN_PATH", "UC_DAEMON_RUN_MODE"} {
		if os.Getenv(key) != "" {
			return fmt.Errorf("the release build refuses the development setting %s", key)
		}
	}
	if os.Getenv("UNICLIPBOARD_ENV") == "development" {
		return fmt.Errorf("the release build refuses UNICLIPBOARD_ENV=development")
	}
	return nil
}

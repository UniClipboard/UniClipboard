//go:build desktop_e2e

package main

import (
	"crypto/sha256"
	"fmt"
	"os"
	"path/filepath"

	"github.com/egoist/mygo"
	_ "github.com/egoist/mygo/plugins/automation"

	"github.com/uniclipboard/unic/internal/clipboardintegration"
	"github.com/uniclipboard/unic/internal/testboard"
)

func init() {
	profile := os.Getenv("UNIC_DESKTOP_E2E_PROFILE")
	canonical, err := filepath.EvalSymlinks(profile)
	if err != nil || !filepath.IsAbs(profile) || canonical != profile || os.Getenv("UNIC_HOME") != profile || os.Getenv("MYGO_AUTOMATION") != filepath.Join(profile, "automation.sock") {
		panic("desktop_e2e requires its isolated canonical profile and automation socket")
	}
	if marker, err := os.ReadFile(filepath.Join(profile, "e2e-owner")); err != nil || string(marker) != "desktop-e2e" {
		panic("desktop_e2e profile has no ownership marker")
	}
	if size := os.Getenv("UNIC_DESKTOP_E2E_WINDOW"); size != "" {
		var width, height int
		if _, err := fmt.Sscanf(size, "%dx%d", &width, &height); err != nil || width < 600 || height < 480 {
			panic("invalid E2E window geometry")
		}
		mygo.Theme.SetSource(mygo.ThemeSource(os.Getenv("UNIC_DESKTOP_E2E_THEME")))
		desktopWindowWidth, desktopWindowHeight = width, height
	}
	sum := sha256.Sum256([]byte(profile))
	desktopAppName = fmt.Sprintf("unic E2E %x", sum[:6])
	newDesktopBoard = func() clipboardintegration.Board { return testboard.Dir(filepath.Join(profile, "board")) }
}

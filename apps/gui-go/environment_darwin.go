//go:build darwin

package main

import (
	"fmt"
	"os"
	"os/user"
	"path/filepath"
	"strings"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/apppaths"
)

// validateIsolation enforces the macOS test mode: throwaway HOME, gui-go-* profile, no system clipboard, and a
// data root that stays inside that HOME. The sandbox is the HOME because the data root, caches and the file-based
// development keystore all resolve beneath it.
func validateIsolation() error {
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
	if os.Getenv("UC_PORTABLE") != "" || apppaths.IsPortable() {
		return fmt.Errorf("the Go GUI refuses portable mode")
	}
	root, ok := apppaths.AppDataRoot()
	if !ok {
		return fmt.Errorf("data root unavailable")
	}
	return ensureInside(root, home)
}

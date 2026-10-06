//go:build windows || linux

package main

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/apppaths"
)

// validateIsolation enforces the Windows and Linux test mode. Neither this host nor the Rust daemon resolve the data root
// from HOME or LOCALAPPDATA: both ask the shell for the known folder, so a temporary HOME would not move anything,
// and the daemon keeps its encryption key in Credential Manager on Windows (on Linux it uses the Secret Service whenever
// DISPLAY and a session bus exist, which an Xvfb test has). Only the file-based keystore of portable mode is
// outside the real user's secrets on both. The sandbox is therefore a portable installation: a throwaway uc-gui-go-*
// directory holding the executables, in which the data root, the caches and the file keystore all live.
func validateIsolation() error {
	exe, err := os.Executable()
	if err != nil {
		return err
	}
	if exe, err = filepath.EvalSymlinks(exe); err != nil {
		return err
	}
	sandbox := filepath.Dir(exe)
	if !strings.HasPrefix(filepath.Base(sandbox), "uc-gui-go-") ||
		!strings.HasPrefix(os.Getenv("UC_PROFILE"), "gui-go-") ||
		os.Getenv("UC_DISABLE_SYSTEM_CLIPBOARD") != "1" || os.Getenv("UC_PORTABLE") != "1" || !apppaths.IsPortable() {
		return fmt.Errorf("isolated mode requires the executable in a uc-gui-go-* directory, UC_PORTABLE=1, a gui-go-* profile and UC_DISABLE_SYSTEM_CLIPBOARD=1")
	}
	root, ok := apppaths.AppDataRoot()
	if !ok {
		return fmt.Errorf("data root unavailable")
	}
	return ensureInside(root, sandbox)
}

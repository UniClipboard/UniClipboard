//go:build windows

package update

import (
	"fmt"
	"os"
	"path/filepath"
	"regexp"

	"golang.org/x/sys/windows"
)

var unsafeNameChars = regexp.MustCompile(`[^A-Za-z0-9._-]`)

// InstallWindows hands a verified payload to the NSIS installer, the way tauri-plugin-updater does: the setup
// program is written to a fresh temporary directory and started with ShellExecute (`/P /R /UPDATE /ARGS ...`).
// The installer stops the old application, replaces the files and starts the new one itself (`/R`), so the caller
// only has to exit afterwards; it must not start a second copy. The temporary directory is left for the installer
// (it is still running from there); the system temp cleanup removes it.
func InstallWindows(payload *os.File, version string, currentArgs []string) error {
	dir, err := os.MkdirTemp("", "UniClipboard-"+unsafeNameChars.ReplaceAllString(version, "_")+"-updater-")
	if err != nil {
		return fmt.Errorf("stage update: %w", err)
	}
	transferred := false
	defer func() {
		if !transferred {
			_ = os.RemoveAll(dir)
		}
	}()
	setup := filepath.Join(dir, "UniClipboard-setup.exe")
	staged, err := os.OpenFile(setup, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0o700)
	if err != nil {
		return fmt.Errorf("stage update: %w", err)
	}
	extractErr := ExtractInstaller(payload, staged)
	closeErr := staged.Close()
	if extractErr != nil {
		return extractErr
	}
	if closeErr != nil {
		return closeErr
	}
	file, err := windows.UTF16PtrFromString(setup)
	if err != nil {
		return err
	}
	params, err := windows.UTF16PtrFromString(NSISArgs(currentArgs))
	if err != nil {
		return err
	}
	if err := windows.ShellExecute(0, windows.StringToUTF16Ptr("open"), file, params, nil, windows.SW_SHOW); err != nil {
		_ = os.RemoveAll(dir)
		return fmt.Errorf("start the installer: %w", err)
	}
	transferred = true
	return nil
}

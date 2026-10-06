//go:build windows

package main

import (
	"errors"
	"fmt"
	"log"
	"strings"

	"golang.org/x/sys/windows/registry"
)

const runKeyPath = `Software\Microsoft\Windows\CurrentVersion\Run`

// sweepLegacyRunValue removes the Run value named after this login item when it launches another executable.
// The Tauri shell (tauri-plugin-autostart / auto-launch) registered `UniClipboard` for its own binary; Wails finds
// its registration by executable path, so a value for a different path is invisible to it and would start the app a
// second time at login. A value that already points at this executable is kept: that is the in-place upgrade case,
// where Wails recognises the old entry and adopts it. Registry value names are case-insensitive. The profile name
// is part of the login item name, so a named profile never reads the primary entry.
func (p loginItemPolicy) sweepLegacyRunValue() error {
	key, err := registry.OpenKey(registry.CURRENT_USER, runKeyPath, registry.QUERY_VALUE|registry.SET_VALUE)
	if errors.Is(err, registry.ErrNotExist) {
		return nil
	}
	if err != nil {
		return fmt.Errorf("open the Run key: %w", err)
	}
	defer key.Close()
	command, _, err := key.GetStringValue(p.name())
	if errors.Is(err, registry.ErrNotExist) {
		return nil
	}
	if err != nil {
		return fmt.Errorf("read the legacy Run value: %w", err)
	}
	if strings.EqualFold(firstCommandToken(command), p.Executable) {
		return nil
	}
	if err := key.DeleteValue(p.name()); err != nil && !errors.Is(err, registry.ErrNotExist) {
		return fmt.Errorf("delete the legacy Run value: %w", err)
	}
	log.Printf("removed the legacy Run value %s (pointed at %s)", p.name(), firstCommandToken(command))
	return nil
}

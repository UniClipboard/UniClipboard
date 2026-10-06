//go:build linux

package main

import (
	"os"
	"strings"
)

// usesCompositorShortcuts follows the Tauri contract (`uses_compositor_shortcuts`): under Wayland the user can
// always bind `uniclipboard --quick-panel` in the compositor, and the settings page says so. Wails also asks the
// GlobalShortcuts portal for the configured accelerator on Wayland, but that request is asynchronous (the portal
// backend's register returns before the compositor answers and reports failures only through the application error
// handler) and the compositor, not Uni, picks the final keys, so a registered shortcut cannot be confirmed here and
// the instruction is shown regardless.
func usesCompositorShortcuts() bool {
	switch strings.ToLower(os.Getenv("XDG_SESSION_TYPE")) {
	case "wayland":
		return true
	case "x11":
		return false
	}
	return os.Getenv("WAYLAND_DISPLAY") != ""
}

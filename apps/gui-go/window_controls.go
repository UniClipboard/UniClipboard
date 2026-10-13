package main

import (
	"os"
	"runtime"
	"strings"
)

// The compositor owns window actions on Hyprland and niri desktops.
func windowControlsScript() string {
	if runtime.GOOS != "linux" {
		return ""
	}
	tiling := os.Getenv("HYPRLAND_INSTANCE_SIGNATURE") != "" || os.Getenv("NIRI_SOCKET") != ""
	for _, name := range strings.FieldsFunc(strings.ToLower(os.Getenv("XDG_CURRENT_DESKTOP")), func(r rune) bool {
		return r == ':' || r == ';'
	}) {
		tiling = tiling || name == "hyprland" || name == "niri"
	}
	if !tiling {
		return ""
	}
	return `window.__UC_WINDOW_CONTROLS_HIDDEN__ = true; window.dispatchEvent(new CustomEvent('uniclipboard:window-frame-changed'));`
}

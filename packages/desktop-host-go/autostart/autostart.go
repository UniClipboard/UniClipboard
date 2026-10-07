// Package autostart registers the desktop app to launch at login. On macOS it writes the same
// LaunchAgent as tauri-plugin-autostart (label and file `<product name>.plist`, run at load, the
// canonical executable path plus the `--autostart` marker), so the Tauri and Go shells share one
// login item instead of creating two.
package autostart

import "errors"

// LaunchArg tags launches started by the login item.
const LaunchArg = "--autostart"

// ErrUnsupported is returned on platforms without an implementation yet.
var ErrUnsupported = errors.New("launch at login is not supported on this platform yet")

// Registration identifies the login item.
type Registration struct {
	// Name is the product name; it names the login item.
	Name string
	// Executable is the absolute path launched at login.
	Executable string
}

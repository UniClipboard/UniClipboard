//go:build linux

package main

import (
	"errors"
	"sync"

	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/hyprland"
	"github.com/wailsapp/wails/v3/pkg/application"
)

// Pasting into the previously focused application on Linux follows the Tauri shell
// (crates/uc-tauri/src/quick_panel/linux.rs): only Hyprland offers a sanctioned way to learn which window had the
// focus, to give it back and to send it a key. Under X11 or any other compositor there is no such path, and the
// Tauri shell reports that instead of pretending, so does this one (the panel is shown again with the error).
const previousAppInputSupported = true

// Hyprland returns the focus to the previous window by itself when the panel hides, so dismissing only hides.
const dismissRestoresPrevious = false

var (
	errNeedsHyprland   = errors.New("Automatic Wayland paste requires Hyprland")
	errNoPreviousApp   = errors.New("No previous application window is available")
	errTextInputLinux  = errors.New("Direct file path input is not yet supported on this platform")
	hyprlandCurrent    = hyprland.Current
	previousHyprWindow struct {
		sync.Mutex
		target *hyprland.WindowTarget
	}
)

// rememberPreviousForeground records the Hyprland active window before the panel takes the focus. Failure to read
// it is not fatal: the paste later reports that there is no previous window.
func rememberPreviousForeground(application.Window) {
	var target *hyprland.WindowTarget
	if client := hyprlandCurrent(); client != nil {
		if active, err := client.ActiveWindow(); err == nil {
			target = active
		}
	}
	previousHyprWindow.Lock()
	previousHyprWindow.target = target
	previousHyprWindow.Unlock()
}

// forceForegroundWindow: there is no foreground lock to defeat on Linux, a plain focus request is the contract.
func forceForegroundWindow(w application.Window) { focusWindow(w) }

// restorePreviousForeground validates the remembered window and focuses it; the target stays remembered so the
// following paste keystroke addresses the same window.
func restorePreviousForeground() error {
	client := hyprlandCurrent()
	if client == nil {
		return errPreviousAppUnsupported
	}
	previousHyprWindow.Lock()
	target := previousHyprWindow.target
	previousHyprWindow.Unlock()
	if target == nil {
		return errNoPreviousApp
	}
	return client.Focus(*target)
}

func simulatePaste() error {
	client := hyprlandCurrent()
	if client == nil {
		return errNeedsHyprland
	}
	previousHyprWindow.Lock()
	target := previousHyprWindow.target
	previousHyprWindow.Unlock()
	if target == nil {
		return errNoPreviousApp
	}
	return client.SendPaste(*target)
}

func simulateTextInput(string) error { return errTextInputLinux }

func runOnMainThread(fn func() error) error { return fn() }

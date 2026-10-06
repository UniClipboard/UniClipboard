//go:build !windows

package main

import (
	"github.com/wailsapp/wails/v3/pkg/application"
)

// Pasting into the previously focused application is implemented for the WebView quick panel on Windows only.
// macOS pastes through the native panel helper (which owns the window and the keystroke); Linux follows in its own
// slice. Every entry point reports that instead of pretending to succeed.
const previousAppInputSupported = false

func rememberPreviousForeground(application.Window) {}
func forceForegroundWindow(application.Window)      {}
func restorePreviousForeground() error              { return errPreviousAppUnsupported }
func simulatePaste() error                          { return errPreviousAppUnsupported }
func simulateTextInput(string) error                { return errPreviousAppUnsupported }
func runOnMainThread(fn func() error) error         { return fn() }

//go:build !windows && !linux

package main

import "errors"

// modifierDoubleTapAvailable: the WebView panel's modifier trigger exists on Windows and on native X11. On macOS the native
// panel helper owns the trigger.
const modifierDoubleTapAvailable = false

func newPlatformKeyState() (modifierKeyState, error) {
	return nil, errors.New("modifier double-tap is not available with this quick panel on this platform yet")
}

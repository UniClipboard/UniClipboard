//go:build !windows

package main

import "errors"

// modifierDoubleTapAvailable: the WebView panel's modifier trigger exists only on Windows. On macOS the native
// panel helper owns the trigger, and Linux (X11) is slice 17c.
const modifierDoubleTapAvailable = false

func newPlatformKeyState() (modifierKeyState, error) {
	return nil, errors.New("modifier double-tap is not available with this quick panel on this platform yet")
}

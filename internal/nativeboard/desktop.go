package nativeboard

import "errors"

// ErrDesktopClipboardUnsupported means the daemon cannot access a desktop clipboard in its session.
var ErrDesktopClipboardUnsupported = errors.New("background clipboard is unavailable in this desktop session")

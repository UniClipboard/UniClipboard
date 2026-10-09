//go:build windows

package main

import "github.com/wailsapp/wails/v3/pkg/application"

// suppressKeyboardMenu keeps a plain Alt (or F10) press from putting the window into the system keyboard-menu mode.
// The windows have no menu bar, yet DefWindowProc still runs a modal menu loop on SC_KEYMENU: it eats the next key
// press, and it keeps the process message loop busy so a quit requested meanwhile never completes. Wails swallows
// SC_KEYMENU when a binding exists for F10, so a no-op binding is enough.
func suppressKeyboardMenu(w *application.WebviewWindow) {
	w.RegisterKeyBinding("f10", func(application.Window) {})
}

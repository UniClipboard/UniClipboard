package main

import (
	"path/filepath"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/apppaths"
)

// webviewDataPath is the WebView2 user data folder (localStorage, cache) for this launch. Without one, Wails uses
// %APPDATA%\<exe name> for every launch of the same exe, so profiles and portable builds shared one store and
// a portable build wrote outside its folder. A named profile or a portable build keeps the store in its own data
// root; the default installed profile keeps the existing folder, so its stored preferences carry over. Empty means
// the Wails default. Only the Windows host reads it.
func webviewDataPath() string {
	if _, named := apppaths.Profile(); !named && !apppaths.IsPortable() {
		return ""
	}
	root, ok := apppaths.AppDataRoot()
	if !ok {
		return ""
	}
	return filepath.Join(root, "webview2")
}

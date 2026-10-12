//go:build !darwin || !private_mac_apis

package windowmaterial

// WebviewTransparent: without -tags private_mac_apis Wails has no public way to clear the WKWebView
// background, so the page stays opaque above any window material.
const WebviewTransparent = false

// Applied reports None: the page covers any window material (or the system has none).
func Applied() string { return None }

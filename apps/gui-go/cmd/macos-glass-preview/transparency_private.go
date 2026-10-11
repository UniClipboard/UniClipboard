//go:build darwin && private_mac_apis

package main

// Built with -tags private_mac_apis: Wails clears the WKWebView background (private drawsBackground
// key), so the window material shows through transparent parts of the page.
const webviewTransparent = true

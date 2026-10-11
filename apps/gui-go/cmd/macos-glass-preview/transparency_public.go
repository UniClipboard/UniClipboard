//go:build darwin && !private_mac_apis

package main

// Public-API build: Wails has no public way to clear the WKWebView background, so the page stays
// opaque above the window material.
const webviewTransparent = false

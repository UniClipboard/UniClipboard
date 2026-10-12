// Package windowmaterial decides which macOS window material the host puts behind a web page, and
// reports it to the page as the `window-material` launch parameter (read by
// frontend/src/host/window-material.ts). The main window uses Wails' frosted NSVisualEffectView,
// visible only when Wails makes the webview transparent with -tags private_mac_apis.
package windowmaterial

// Values of the `window-material` launch parameter.
const (
	LiquidGlass  = "liquid-glass"
	VisualEffect = "visual-effect"
	None         = "none"
)

// LaunchParameter is the query parameter that carries the applied material to the page.
const LaunchParameter = "window-material"

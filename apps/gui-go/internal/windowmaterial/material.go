// Package windowmaterial decides which macOS window material the host puts behind a web page, and
// reports it to the page as the `window-material` launch parameter (read by
// frontend/src/host/window-material.ts). It follows the checks of windowSetLiquidGlass in the pinned
// Wails release: NSGlassEffectView needs macOS 26, older systems get NSVisualEffectView, and either is
// only visible when the webview is transparent, which Wails allows only with -tags private_mac_apis.
package windowmaterial

import (
	"runtime"
	"strconv"
	"strings"
	"syscall"
)

// Values of the `window-material` launch parameter.
const (
	LiquidGlass  = "liquid-glass"
	VisualEffect = "visual-effect"
	None         = "none"
)

// LaunchParameter is the query parameter that carries the applied material to the page.
const LaunchParameter = "window-material"

// Applied reports the material the host applies to a window created with Wails'
// MacBackdropLiquidGlass, or None when the page would cover it (or on other systems).
func Applied() string {
	if runtime.GOOS != "darwin" || !WebviewTransparent {
		return None
	}
	version, err := syscall.Sysctl("kern.osproductversion")
	if err != nil {
		return VisualEffect
	}
	major, err := strconv.Atoi(strings.SplitN(version, ".", 2)[0])
	if err == nil && major >= 26 {
		return LiquidGlass
	}
	return VisualEffect
}

//go:build darwin && private_mac_apis

package windowmaterial

import (
	"strconv"
	"strings"
	"syscall"
)

// WebviewTransparent: built with -tags private_mac_apis, Wails clears the WKWebView background
// (private `drawsBackground` key), so the window material shows through transparent parts of the page.
const WebviewTransparent = true

// Applied reports the material the host applies to a window created with Wails'
// MacBackdropLiquidGlass: NSGlassEffectView from macOS 26, NSVisualEffectView before.
func Applied() string {
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

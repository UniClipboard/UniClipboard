//go:build darwin

package main

import (
	"net/url"

	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/windowmaterial"
	"github.com/wailsapp/wails/v3/pkg/application"
)

// withWindowMaterial puts the pinned Wails release's window material (MacBackdropLiquidGlass:
// NSGlassEffectView on macOS 26+, NSVisualEffectView before) behind the main window and tells the
// page which one it got through the `window-material` launch parameter. When the webview cannot be
// transparent (a build without -tags private_mac_apis) no material is applied and the page reports
// `none`, so it stays opaque. The window keeps the system appearance: Wails beta.28 sets
// NSAppearance only at creation, and the page falls back to an opaque sidebar when the app theme
// differs from it (src/ui-kit/skins/macos/root.tsx).
func withWindowMaterial(o application.WebviewWindowOptions) application.WebviewWindowOptions {
	material := windowmaterial.Applied()
	o.URL += "?" + url.Values{windowmaterial.LaunchParameter: {material}}.Encode()
	if material == windowmaterial.None {
		return o
	}
	o.BackgroundType = application.BackgroundTypeTransparent
	o.BackgroundColour = application.RGBA{}
	o.Mac.Backdrop = application.MacBackdropLiquidGlass
	o.Mac.LiquidGlass = application.MacLiquidGlass{
		Style:    application.LiquidGlassStyleAutomatic,
		Material: application.NSVisualEffectMaterialSidebar,
	}
	return o
}

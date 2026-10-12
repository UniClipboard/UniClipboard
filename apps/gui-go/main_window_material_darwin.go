//go:build darwin

package main

import (
	"net/url"

	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/windowmaterial"
	"github.com/wailsapp/wails/v3/pkg/application"
)

// withWindowMaterial puts Wails' frosted NSVisualEffectView behind the main window and tells the
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
	o.Mac.Backdrop = application.MacBackdropTranslucent
	return o
}

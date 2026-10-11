//go:build darwin

// Command macos-glass-preview opens the macOS design prototype in a real Wails window with the
// window material of the pinned Wails release (MacBackdropLiquidGlass: NSGlassEffectView on macOS 26+,
// NSVisualEffectView before). It is a development preview only: it is not part of the product
// build, reads no profile or daemon, and proxies the page from a running Vite dev server.
//
//	cd apps/gui-go/frontend && UC_DEV_SERVER_PORT=1531 npx vite
//	cd apps/gui-go && go run -tags private_mac_apis ./cmd/macos-glass-preview
//
// Without -tags private_mac_apis Wails keeps the WKWebView opaque (there is no public API to make it
// transparent), so the native material exists but the page covers it; the page is told so through
// the `window-material` launch parameter and draws its CSS fallback instead.
package main

import (
	"flag"
	"log"
	"net/http/httputil"
	"net/url"
	"strconv"
	"strings"
	"syscall"

	"github.com/wailsapp/wails/v3/pkg/application"
)

// appliedMaterial reports what Wails actually puts behind the page, following the checks of
// windowSetLiquidGlass in the pinned Wails source: NSGlassEffectView needs macOS 26, older systems get
// NSVisualEffectView, and either is only visible when the webview is transparent.
func appliedMaterial() string {
	if !webviewTransparent {
		return "none"
	}
	version, err := syscall.Sysctl("kern.osproductversion")
	if err != nil {
		return "visual-effect"
	}
	major, err := strconv.Atoi(strings.SplitN(version, ".", 2)[0])
	if err == nil && major >= 26 {
		return "liquid-glass"
	}
	return "visual-effect"
}

func separator(path string) string {
	if strings.Contains(path, "?") {
		return "&"
	}
	return "?"
}

func main() {
	devServer := flag.String("dev-server", "http://127.0.0.1:1531", "Vite dev server that serves the page")
	path := flag.String("path", "/macos-design-preview.html", "page to open; it receives the host launch parameters")
	generation := flag.String("generation", "27", "macOS design generation of the window layout (26 or 27)")
	theme := flag.String("theme", "system", "light, dark or system")
	width := flag.Int("width", 1100, "initial window width")
	height := flag.Int("height", 720, "initial window height")
	flag.Parse()

	target, err := url.Parse(*devServer)
	if err != nil {
		log.Fatal(err)
	}
	material := appliedMaterial()
	log.Printf("macOS glass preview: webview transparent=%v material=%s", webviewTransparent, material)

	app := application.New(application.Options{
		Name: "macOS Glass Preview",
		Mac:  application.MacOptions{ApplicationShouldTerminateAfterLastWindowClosed: true},
		// The page comes from the dev server through the Wails asset origin, so the Wails runtime is present.
		Assets: application.AssetOptions{Handler: httputil.NewSingleHostReverseProxy(target)},
	})

	// The glass draws in the window's NSAppearance, so it must match the page theme. Wails beta.28 sets
	// it only at window creation; "system" leaves it to AppKit, which follows system changes live.
	appearance := map[string]application.MacAppearanceType{
		"light": application.NSAppearanceNameAqua,
		"dark":  application.NSAppearanceNameDarkAqua,
	}[*theme]

	// `window-material` is the host launch parameter read by src/host/window-material.ts.
	query := url.Values{"shell": {"native"}, "window-material": {material}, "generation": {*generation}, "theme": {*theme}}
	app.Window.NewWithOptions(application.WebviewWindowOptions{
		Title:            "UniClipboard — macOS glass preview",
		URL:              *path + separator(*path) + query.Encode(),
		Width:            *width,
		Height:           *height,
		MinWidth:         760,
		MinHeight:        480,
		BackgroundType:   application.BackgroundTypeTransparent,
		BackgroundColour: application.RGBA{Red: 0, Green: 0, Blue: 0, Alpha: 0},
		Mac: application.MacWindow{
			TitleBar:   application.MacTitleBarHiddenInset,
			Appearance: appearance,
			Backdrop:   application.MacBackdropLiquidGlass,
			LiquidGlass: application.MacLiquidGlass{
				Style:    application.LiquidGlassStyleAutomatic,
				Material: application.NSVisualEffectMaterialSidebar,
			},
		},
	})

	if err := app.Run(); err != nil {
		log.Fatal(err)
	}
}

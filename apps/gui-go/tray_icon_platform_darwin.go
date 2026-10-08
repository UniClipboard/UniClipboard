package main

import (
	"image/color"

	"github.com/wailsapp/wails/v3/pkg/application"
)

// macOS gives the status item a square of the menu bar's thickness (22 pt; Wails sets it) and draws a template image in the menu bar's own
// colour from its alpha alone. 44 px is the 2x pixel size, which every current Mac shows at most.
//
// The glyph fills 18 x 20 of the 24 grid units, so drawing the grid at 22 pt (44 px) makes it about 16.5 x 18.3 pt, which matches the size of
// neighbouring status items (the design's own 18 pt grid would make it 13.5 x 15 pt).
const (
	macTrayImagePx = 44
	macTrayArtPx   = 44 // 22 pt at 2x
)

func trayIconSpec() iconSpec {
	return iconSpec{size: macTrayImagePx, art: macTrayArtPx, pal: iconPalette{fg: color.NRGBA{A: 255}}} // only alpha matters for a template image
}

func applyTrayIcon(tray *application.SystemTray, data []byte) { tray.SetTemplateIcon(data) }

// watchSystemTheme: macOS recolours a template image itself, so nothing to watch.
func watchSystemTheme(*application.App, func()) {}

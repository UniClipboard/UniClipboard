package main

import (
	"image/color"

	"github.com/wailsapp/wails/v3/pkg/application"
)

// macOS gives the status item a square of the menu bar's thickness (22 pt; Wails sets it) and draws a template image in the menu bar's own
// colour from its alpha alone. 44 px is the 2x pixel size, which every current Mac shows at most.
//
// The design draws the 24-unit grid at 18 pt, but the cat only fills about 74% of the grid, so it showed 14 x 13 pt next to neighbouring
// status items that fill 17 to 20 pt. The grid is drawn at 24 pt (48 px) instead: the cat is then 18 x 17 pt and only empty margin of the
// grid falls outside the 22 pt square.
const macTrayImagePx = 44

// macTrayArtPx is the pixel size the 24-unit grid is drawn at, chosen per design so the visible glyph is about 17 to 18 pt tall.
func macTrayArtPx(d trayDesign) float64 {
	if d == designGlyph {
		return 44 // the cards fill 18 x 20 of the 24 units: 16.5 x 18.3 pt
	}
	return 48 // 24 pt at 2x
}

func trayIconSpec() iconSpec { return trayIconSpecFor(designFromEnv()) }

func trayIconSpecFor(d trayDesign) iconSpec {
	return iconSpec{design: d, size: macTrayImagePx, art: macTrayArtPx(d), pal: iconPalette{fg: color.NRGBA{A: 255}}} // only alpha matters for a template image
}

func applyTrayIcon(tray *application.SystemTray, data []byte) { tray.SetTemplateIcon(data) }

// watchSystemTheme: macOS recolours a template image itself, so nothing to watch.
func watchSystemTheme(*application.App, func()) {}

package main

import (
	"image"
	"image/color"
)

// The tray icon is the "one glyph" design of the Claude Design canvas (docs/architecture/gui-go-tray-icon.md): two offset cards, a stroke of
// width 2 with round caps and joins on the 24 x 24 grid (boards "TrayIconBuild" and "TrayIconStates"). The path data is copied from the
// design's SVG. Only its resting "synced" form is drawn; the design's other states and motion are not implemented.
const (
	glyphStrokeWidth = 2.0
	glyphBackPath    = "M4 7.5v10A3.5 3.5 0 0 0 7.5 21H15" // the back card: left and bottom edges
)

// Parsed once: the design's paths never change.
var (
	glyphFront = mustPath(roundRectPath(7, 3, 13, 15, 3.5)) // the front card
	glyphBack  = mustPath(glyphBackPath)
)

// iconPalette is the foreground colour a platform draws the glyph with.
type iconPalette struct{ fg color.NRGBA }

// iconSpec is one image: its pixel geometry and colour.
type iconSpec struct {
	size int     // image edge in pixels
	art  float64 // edge of the 24-unit design grid in pixels; the grid is centred in the image
	pal  iconPalette
}

func mustPath(d string) []subpath {
	paths, err := parsePath(d)
	if err != nil {
		panic("tray icon design path: " + err.Error())
	}
	return paths
}

func renderIcon(s iconSpec) *image.NRGBA {
	c := newCanvas(s.size, s.art)
	c.paintStroke(strokePolys(glyphFront, glyphStrokeWidth), s.pal.fg, 1)
	c.paintStroke(strokePolys(glyphBack, glyphStrokeWidth), s.pal.fg, 1)
	return c.image()
}

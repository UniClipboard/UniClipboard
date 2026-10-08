package main

import (
	"image"
	"image/color"
	"os"
)

// The tray icon is drawn from one of two designs of the same Claude Design canvas (docs/architecture/gui-go-tray-icon.md), each only in its
// resting "synced" form, so they can be compared: the "B solid cat" (ears down, two knocked-out eyes) and the "one glyph" (two offset
// cards). The path strings are copied from the design boards' SVG source. The designs' other states and the ear motion are not implemented.

const (
	catFacePath = "M12 6.4c-4.9 0-8.9 3-8.9 7.2 0 4.1 3.8 6.6 8.9 6.6s8.9-2.5 8.9-6.6c0-4.2-4-7.2-8.9-7.2z"
	catEarLPath = "M5.2 10.6 4.3 4.4c-.1-.8.6-1.2 1.2-.8l4.9 3.4z"
	catEarRPath = "M18.8 10.6l.9-6.2c.1-.8-.6-1.2-1.2-.8l-4.9 3.4z"
)

// The one-glyph design: two offset cards drawn as a stroke of width 2 with round caps and joins (board "TrayIconBuild").
const (
	glyphStrokeWidth = 2.0
	glyphBackPath    = "M4 7.5v10A3.5 3.5 0 0 0 7.5 21H15"
)

// Eyes are knocked out (transparent), an ellipse of rx 1.3 and ry 1.6 at (9, 13.8) and (15, 13.8).
const (
	eyeRX, eyeRY        = 1.3, 1.6
	eyeY                = 13.8
	eyeLeftX, eyeRightX = 9.0, 15.0
)

// Parsed once: the design's paths never change.
var (
	catFace    = mustPath(catFacePath)
	catEarL    = mustPath(catEarLPath)
	catEarR    = mustPath(catEarRPath)
	glyphFront = mustPath(roundRectPath(7, 3, 13, 15, 3.5))
	glyphBack  = mustPath(glyphBackPath)
	catEyes    = mustPath(ellipsePath(eyeLeftX, eyeY, eyeRX, eyeRY) + ellipsePath(eyeRightX, eyeY, eyeRX, eyeRY))
)

// trayDesign selects what is drawn.
type trayDesign int

const (
	designGlyph trayDesign = iota // the "one glyph, nine states" cards
	designCat                     // the "B solid cat"
)

// designFromEnv picks the design for the running process: the cards by default, UC_TRAY_ICON=cat selects the cat. It exists so the two can
// be compared on a real menu bar until one is chosen.
func designFromEnv() trayDesign {
	if os.Getenv("UC_TRAY_ICON") == "cat" {
		return designCat
	}
	return designGlyph
}

// iconPalette is the foreground colour a platform draws the cat with.
type iconPalette struct{ fg color.NRGBA }

// iconSpec is one image: its pixel geometry and colour.
type iconSpec struct {
	design trayDesign
	size   int     // image edge in pixels
	art    float64 // edge of the 24-unit design grid in pixels; the grid is centred in the image
	pal    iconPalette
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
	if s.design == designGlyph {
		c.paint(strokePolys(glyphFront, glyphStrokeWidth, true), s.pal.fg, 1)
		c.paint(strokePolys(glyphBack, glyphStrokeWidth, true), s.pal.fg, 1)
		return c.image()
	}
	c.paint(fillPolys(catEarL), s.pal.fg, 1)
	c.paint(fillPolys(catEarR), s.pal.fg, 1)
	c.paint(fillPolys(catFace), s.pal.fg, 1)
	c.erase(fillPolys(catEyes))
	return c.image()
}

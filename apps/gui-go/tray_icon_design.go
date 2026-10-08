package main

import (
	"image"
	"image/color"
)

// The tray icon is the "B solid cat" design (docs/architecture/gui-go-tray-icon.md). The path strings below are copied from the design
// boards' SVG source; the stack order follows the SVG element order. Only the resting cat ("synced": ears down, two knocked-out eyes) is
// drawn; the design's other states and the ear motion are not implemented.

const (
	catFacePath = "M12 6.4c-4.9 0-8.9 3-8.9 7.2 0 4.1 3.8 6.6 8.9 6.6s8.9-2.5 8.9-6.6c0-4.2-4-7.2-8.9-7.2z"
	catEarLPath = "M5.2 10.6 4.3 4.4c-.1-.8.6-1.2 1.2-.8l4.9 3.4z"
	catEarRPath = "M18.8 10.6l.9-6.2c.1-.8-.6-1.2-1.2-.8l-4.9 3.4z"
)

// Eyes are knocked out (transparent), an ellipse of rx 1.3 and ry 1.6 at (9, 13.8) and (15, 13.8).
const (
	eyeRX, eyeRY        = 1.3, 1.6
	eyeY                = 13.8
	eyeLeftX, eyeRightX = 9.0, 15.0
)

// Parsed once: the design's paths never change.
var (
	catFace = mustPath(catFacePath)
	catEarL = mustPath(catEarLPath)
	catEarR = mustPath(catEarRPath)
	catEyes = mustPath(ellipsePath(eyeLeftX, eyeY, eyeRX, eyeRY) + ellipsePath(eyeRightX, eyeY, eyeRX, eyeRY))
)

// iconPalette is the foreground colour a platform draws the cat with.
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
	c.paint(fillPolys(catEarL), s.pal.fg, 1)
	c.paint(fillPolys(catEarR), s.pal.fg, 1)
	c.paint(fillPolys(catFace), s.pal.fg, 1)
	c.erase(fillPolys(catEyes))
	return c.image()
}

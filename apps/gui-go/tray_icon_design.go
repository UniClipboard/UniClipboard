package main

import (
	"image"
	"image/color"
)

// The tray icon is the "B solid cat" design (docs/architecture/gui-go-tray-icon.md). Every path string below is copied from the
// design boards' SVG source; the stack order of shapeOps follows the SVG element order, so fill and stroke paint exactly as the
// design shows them.

const (
	catFacePath  = "M12 6.4c-4.9 0-8.9 3-8.9 7.2 0 4.1 3.8 6.6 8.9 6.6s8.9-2.5 8.9-6.6c0-4.2-4-7.2-8.9-7.2z"
	catEarLPath  = "M5.2 10.6 4.3 4.4c-.1-.8.6-1.2 1.2-.8l4.9 3.4z"
	catEarRPath  = "M18.8 10.6l.9-6.2c.1-.8-.6-1.2-1.2-.8l-4.9 3.4z"
	sleepingEyes = "M7.6 13.8q1.4 1.5 2.8 0M13.6 13.8q1.4 1.5 2.8 0"
)

var (
	earLPivot = point{7.4, 8.6}
	earRPivot = point{16.6, 8.6}
)

// iconBase is the single state shown on the cat; "new content" is a separate dot that the design lets sit on any of them.
type iconBase int

const (
	baseSynced iconBase = iota
	baseTransferring
	basePaused
	baseLANOnly
	baseOffline
	baseNotRecording
	baseLocked
	baseAttention
)

var iconBaseNames = [...]string{"synced", "transferring", "paused", "lan-only", "offline", "not-recording", "locked", "attention"}

func (b iconBase) String() string { return iconBaseNames[b] }

// role names what a shape is painted with. The design draws knock-outs in the surface colour; here that is an erase.
type role int

const (
	roleNone role = iota
	roleFg
	roleBg       // knock-out: transparent
	roleAccent   // the system accent on Windows, the foreground elsewhere
	roleAlert    // the warning red on Windows, the foreground elsewhere
	roleOnAccent // white on a coloured badge, a knock-out on a monochrome one
)

type shapeOp struct {
	d      string
	fill   role
	stroke role
	width  float64
	round  bool
}

func circleOp(cx, cy, r float64, fill, stroke role, width float64) shapeOp {
	return shapeOp{d: ellipsePath(cx, cy, r, r), fill: fill, stroke: stroke, width: width, round: true}
}

func eyes(rx, ry float64) []shapeOp {
	return []shapeOp{
		{d: ellipsePath(9, 13.8, rx, ry), fill: roleBg},
		{d: ellipsePath(15, 13.8, rx, ry), fill: roleBg},
	}
}

// newContentDot is the dot beside the right ear ("TrayBBuild": centre (21.2, 9), radius 2.3, 2.4 wide knock-out ring).
var newContentDot = circleOp(21.2, 9, 2.3, roleAccent, roleBg, 2.4)

// stateOps returns the shapes drawn over the cat for a state, copied from the "TrayBStates" board.
func stateOps(b iconBase) []shapeOp {
	switch b {
	case baseTransferring:
		return append(eyes(1.3, 1.6),
			circleOp(19, 19, 4.6, roleAccent, roleBg, 2.4),
			shapeOp{d: "M17.6 21v-3.8M16.3 18.5l1.3-1.3 1.3 1.3M20.4 17v3.8M19.1 19.5l1.3 1.3 1.3-1.3", stroke: roleOnAccent, width: 1.1, round: true})
	case basePaused:
		return []shapeOp{
			{d: sleepingEyes, stroke: roleBg, width: 1.4, round: true},
			circleOp(19, 19, 4.6, roleBg, roleBg, 2.4),
			{d: "M17.2 17.2h3.6l-3.6 3.6h3.6", stroke: roleFg, width: 1.3, round: true},
		}
	case baseLANOnly:
		return append(eyes(1.3, 1.6),
			circleOp(19, 19, 4.6, roleBg, roleBg, 2.4),
			shapeOp{d: "M16.6 19.6 19 17.3l2.4 2.3v2h-4.8z", stroke: roleFg, width: 1.2, round: true})
	case baseOffline:
		return append(eyes(1.3, 1.6),
			shapeOp{d: "M3.5 3.5l17 17", stroke: roleBg, width: 4.2, round: true},
			shapeOp{d: "M3.5 3.5l17 17", stroke: roleFg, width: 1.8, round: true})
	case baseNotRecording:
		return []shapeOp{{d: roundRectPath(6.3, 12.6, 11.4, 2.5, 1.25), fill: roleBg}}
	case baseLocked:
		const shackle = "M17 18v-1.3a2 2 0 0 1 4 0V18"
		return append(eyes(1.3, 1.6),
			shapeOp{d: shackle, stroke: roleBg, width: 3.6},
			shapeOp{d: roundRectPath(15.2, 17.8, 7.6, 5.6, 1.4), fill: roleFg, stroke: roleBg, width: 2.4},
			shapeOp{d: shackle, stroke: roleFg, width: 1.3})
	case baseAttention:
		return append(eyes(1.6, 1.95),
			circleOp(19, 19, 4.6, roleAlert, roleBg, 2.4),
			shapeOp{d: "M19 16.9v2.5", stroke: roleOnAccent, width: 1.5, round: true},
			shapeOp{d: ellipsePath(19, 21.2, .8, .8), fill: roleOnAccent})
	default: // baseSynced
		return eyes(1.3, 1.6)
	}
}

// iconPalette is how the roles turn into pixels on one platform and surface.
type iconPalette struct {
	fg       color.NRGBA
	coloured bool // Windows: the badge uses the accent and warning colours; elsewhere the icon is one colour
	accent   color.NRGBA
	alert    color.NRGBA
}

var onAccentWhite = color.NRGBA{255, 255, 255, 255}

// offlineBodyOpacity is the whole cat's opacity in the "offline" state.
const offlineBodyOpacity = 0.42

// iconSpec is one frame: which state, whether the dot is shown, how far each ear is turned, and the pixel geometry.
type iconSpec struct {
	base       iconBase
	dot        bool
	earL, earR float64 // degrees, clockwise on screen; the left ear turns outward with a negative angle
	size       int     // image edge in pixels
	art        float64 // edge of the 24-unit design grid in pixels; the grid is centred in the image
	pal        iconPalette
}

func mustPath(d string) []subpath {
	paths, err := parsePath(d)
	if err != nil {
		panic("tray icon design path: " + err.Error())
	}
	return paths
}

// Parsed once: the design's paths never change.
var (
	catFace = mustPath(catFacePath)
	catEarL = mustPath(catEarLPath)
	catEarR = mustPath(catEarRPath)
)

func renderIcon(s iconSpec) *image.NRGBA {
	c := newCanvas(s.size, s.art)
	c.paint(fillPolys(rotateAbout(catEarL, earLPivot, s.earL)), s.pal.fg, 1)
	c.paint(fillPolys(rotateAbout(catEarR, earRPivot, s.earR)), s.pal.fg, 1)
	c.paint(fillPolys(catFace), s.pal.fg, 1)
	if s.base == baseOffline {
		c.scaleAlpha(offlineBodyOpacity) // group opacity: the ears and the face fade together, not one over the other
	}
	ops := stateOps(s.base)
	if s.dot {
		ops = append(ops, newContentDot)
	}
	for _, op := range ops {
		paths := mustPath(op.d)
		if op.fill != roleNone {
			applyRole(c, s.pal, op.fill, fillPolys(paths))
		}
		if op.stroke != roleNone {
			applyRole(c, s.pal, op.stroke, strokePolys(paths, op.width, op.round))
		}
	}
	return c.image()
}

func applyRole(c *canvas, pal iconPalette, r role, polys [][]point) {
	switch r {
	case roleBg:
		c.erase(polys)
	case roleFg:
		c.paint(polys, pal.fg, 1)
	case roleAccent:
		if pal.coloured {
			c.paint(polys, pal.accent, 1)
		} else {
			c.paint(polys, pal.fg, 1)
		}
	case roleAlert:
		if pal.coloured {
			c.paint(polys, pal.alert, 1)
		} else {
			c.paint(polys, pal.fg, 1)
		}
	case roleOnAccent:
		if pal.coloured {
			c.paint(polys, onAccentWhite, 1)
		} else {
			c.erase(polys)
		}
	}
}

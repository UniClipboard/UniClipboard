package quickpanel

import "math"

// Quick Panel placement modes.
const (
	PositionCenter = "center"
	PositionCursor = "cursor"
	PositionCaret  = "caret"
)

// NormalizePosition returns a supported placement mode, defaulting to center.
func NormalizePosition(mode string) string {
	switch mode {
	case PositionCursor, PositionCaret:
		return mode
	default:
		return PositionCenter
	}
}

// Point is an integer 2D coordinate.
type Point struct {
	X, Y int
}

// Rect is an integer rectangle with origin (X, Y) and non-negative size.
type Rect struct {
	X, Y, Width, Height int
}

// Display mirrors a screen display in mygo's unified DIP space.
type Display struct {
	ID          int64
	Bounds      Rect
	WorkArea    Rect
	ScaleFactor float64
	Primary     bool
}

// PhysicalMonitor records a monitor's Win32 physical pixel bounds keyed by HMONITOR.
type PhysicalMonitor struct {
	ID     int64
	Bounds Rect
}

// AnchorKind classifies the captured placement anchor.
type AnchorKind uint8

const (
	AnchorNone AnchorKind = iota
	// AnchorCaret is an exact text insertion caret rectangle.
	AnchorCaret
	// AnchorInputControl is a compact single-line input box (e.g. search/address bar)
	// used when an exact caret range is unavailable.
	AnchorInputControl
)

// PlacementAnchor is ephemeral screen geometry captured before the Quick Panel
// takes foreground focus. Never log or persist it.
type PlacementAnchor struct {
	HasCursor bool
	Cursor    Point
	HasWindow bool
	Window    Rect
	Kind      AnchorKind
	Rect      Rect
	Monitors  []PhysicalMonitor
}

// PositionResolver is optionally implemented by platforms that capture native
// cursor, window, and caret geometry before showing the Quick Panel.
type PositionResolver interface {
	CapturePlacement(mode string, target Target) (PlacementAnchor, bool)
}

const (
	placementMarginDIP = 8
	cursorOffsetXDIP   = 8
	cursorOffsetYDIP   = 10
	caretGapYDIP       = 6
)

// ResolvePlacement computes the Quick Panel's top-left corner in mygo's unified
// DIP space for the requested mode.
func ResolvePlacement(mode string, panelW, panelH int, anchor PlacementAnchor, displays []Display) (x, y int, ok bool) {
	if len(displays) == 0 || panelW <= 0 || panelH <= 0 {
		return 0, 0, false
	}
	switch NormalizePosition(mode) {
	case PositionCaret:
		if (anchor.Kind == AnchorCaret || anchor.Kind == AnchorInputControl) && anchor.Rect.Height > 0 {
			if r, disp, matched := PhysicalRectToDIP(anchor.Rect, anchor.Monitors, displays); matched {
				x, y = placeAtCaret(r, panelW, panelH, effectiveWorkArea(disp))
				return x, y, true
			}
		}
		return resolveCursorPlacement(panelW, panelH, anchor, displays)
	case PositionCursor:
		return resolveCursorPlacement(panelW, panelH, anchor, displays)
	default:
		return resolveCenterPlacement(panelW, panelH, anchor, displays)
	}
}

func resolveCenterPlacement(panelW, panelH int, anchor PlacementAnchor, displays []Display) (int, int, bool) {
	disp, ok := selectCenterDisplay(anchor, displays)
	if !ok {
		return 0, 0, false
	}
	work := effectiveWorkArea(disp)
	x := work.X + (work.Width-panelW)/2
	y := work.Y + (work.Height-panelH)/2
	x, y = clampToWorkArea(x, y, panelW, panelH, work, placementMarginDIP)
	return x, y, true
}

func resolveCursorPlacement(panelW, panelH int, anchor PlacementAnchor, displays []Display) (int, int, bool) {
	if anchor.HasCursor {
		if pt, disp, ok := PhysicalPointToDIP(anchor.Cursor, anchor.Monitors, displays); ok {
			x, y := placeAtCursor(pt, panelW, panelH, effectiveWorkArea(disp))
			return x, y, true
		}
	}
	return resolveCenterPlacement(panelW, panelH, anchor, displays)
}

func selectCenterDisplay(anchor PlacementAnchor, displays []Display) (Display, bool) {
	if len(displays) == 0 {
		return Display{}, false
	}
	if anchor.HasWindow && anchor.Window.Width > 0 && anchor.Window.Height > 0 {
		if _, disp, ok := PhysicalRectToDIP(anchor.Window, anchor.Monitors, displays); ok {
			return disp, true
		}
	}
	if anchor.HasCursor {
		if _, disp, ok := PhysicalPointToDIP(anchor.Cursor, anchor.Monitors, displays); ok {
			return disp, true
		}
	}
	for _, d := range displays {
		if d.Primary {
			return d, true
		}
	}
	return displays[0], true
}

func placeAtCursor(cursor Point, panelW, panelH int, work Rect) (int, int) {
	rightLimit := work.X + work.Width - placementMarginDIP
	bottomLimit := work.Y + work.Height - placementMarginDIP
	leftLimit := work.X + placementMarginDIP
	topLimit := work.Y + placementMarginDIP

	x := cursor.X + cursorOffsetXDIP
	if x+panelW > rightLimit {
		altX := cursor.X - cursorOffsetXDIP - panelW
		if altX >= leftLimit || (rightLimit-x) < (altX+panelW-leftLimit) {
			x = altX
		}
	}

	y := cursor.Y + cursorOffsetYDIP
	if y+panelH > bottomLimit {
		altY := cursor.Y - cursorOffsetYDIP - panelH
		if altY >= topLimit || (bottomLimit-y) < (altY+panelH-topLimit) {
			y = altY
		}
	}

	return clampToWorkArea(x, y, panelW, panelH, work, placementMarginDIP)
}

func placeAtCaret(anchor Rect, panelW, panelH int, work Rect) (int, int) {
	bottomLimit := work.Y + work.Height - placementMarginDIP
	topLimit := work.Y + placementMarginDIP

	x := anchor.X
	belowY := anchor.Y + anchor.Height + caretGapYDIP
	y := belowY
	if belowY+panelH > bottomLimit {
		aboveY := anchor.Y - caretGapYDIP - panelH
		spaceBelow := bottomLimit - (anchor.Y + anchor.Height + caretGapYDIP)
		spaceAbove := (anchor.Y - caretGapYDIP) - topLimit
		if aboveY >= topLimit || spaceAbove > spaceBelow {
			y = aboveY
		}
	}
	return clampToWorkArea(x, y, panelW, panelH, work, placementMarginDIP)
}

func clampToWorkArea(x, y, panelW, panelH int, work Rect, margin int) (int, int) {
	mx := margin
	if work.Width <= panelW+2*mx {
		mx = max(0, (work.Width-panelW)/2)
	}
	my := margin
	if work.Height <= panelH+2*my {
		my = max(0, (work.Height-panelH)/2)
	}

	minX := work.X + mx
	maxX := work.X + work.Width - panelW - mx
	if maxX < minX {
		x = work.X
	} else {
		x = max(minX, min(x, maxX))
	}

	minY := work.Y + my
	maxY := work.Y + work.Height - panelH - my
	if maxY < minY {
		y = work.Y
	} else {
		y = max(minY, min(y, maxY))
	}
	return x, y
}

func effectiveWorkArea(d Display) Rect {
	if d.WorkArea.Width > 0 && d.WorkArea.Height > 0 {
		return d.WorkArea
	}
	return d.Bounds
}

// PhysicalPointToDIP converts a physical pixel point into mygo's unified DIP space
// using the matching monitor's physical origin and DIP origin/scale factor.
func PhysicalPointToDIP(pt Point, monitors []PhysicalMonitor, displays []Display) (Point, Display, bool) {
	if len(displays) == 0 {
		return Point{}, Display{}, false
	}
	if len(monitors) == 0 {
		d := nearestDisplayToPoint(pt, displays)
		return pt, d, true
	}
	mon, ok := bestPhysicalMonitor(Rect{X: pt.X, Y: pt.Y, Width: 1, Height: 1}, monitors)
	if !ok {
		return Point{}, Display{}, false
	}
	disp, ok := findDisplayByID(mon.ID, displays)
	if !ok {
		return Point{}, Display{}, false
	}
	dpi := displayDPI(disp)
	return Point{
		X: disp.Bounds.X + floorDiv((pt.X-mon.Bounds.X)*96, dpi),
		Y: disp.Bounds.Y + floorDiv((pt.Y-mon.Bounds.Y)*96, dpi),
	}, disp, true
}

// PhysicalRectToDIP converts a physical pixel rectangle into mygo's unified DIP
// space on the monitor that overlaps it the most (or nearest).
func PhysicalRectToDIP(r Rect, monitors []PhysicalMonitor, displays []Display) (Rect, Display, bool) {
	if len(displays) == 0 {
		return Rect{}, Display{}, false
	}
	if len(monitors) == 0 {
		d := nearestDisplayToPoint(Point{X: r.X + r.Width/2, Y: r.Y + r.Height/2}, displays)
		return r, d, true
	}
	mon, ok := bestPhysicalMonitor(r, monitors)
	if !ok {
		return Rect{}, Display{}, false
	}
	disp, ok := findDisplayByID(mon.ID, displays)
	if !ok {
		return Rect{}, Display{}, false
	}
	dpi := displayDPI(disp)
	return Rect{
		X:      disp.Bounds.X + floorDiv((r.X-mon.Bounds.X)*96, dpi),
		Y:      disp.Bounds.Y + floorDiv((r.Y-mon.Bounds.Y)*96, dpi),
		Width:  scaledInt(r.Width, 96, dpi),
		Height: scaledInt(r.Height, 96, dpi),
	}, disp, true
}

func displayDPI(d Display) int {
	if d.ScaleFactor <= 0 {
		return 96
	}
	dpi := int(math.Round(d.ScaleFactor * 96))
	if dpi <= 0 {
		return 96
	}
	return dpi
}

func findDisplayByID(id int64, displays []Display) (Display, bool) {
	for _, d := range displays {
		if d.ID == id {
			return d, true
		}
	}
	return Display{}, false
}

func nearestDisplayToPoint(pt Point, displays []Display) Display {
	best := displays[0]
	bestDist := int64(math.MaxInt64)
	for _, d := range displays {
		dist := rectPointDistSq(d.Bounds, pt)
		if dist < bestDist {
			best, bestDist = d, dist
		}
	}
	return best
}

func bestPhysicalMonitor(r Rect, monitors []PhysicalMonitor) (PhysicalMonitor, bool) {
	if len(monitors) == 0 {
		return PhysicalMonitor{}, false
	}
	rw, rh := max(r.Width, 1), max(r.Height, 1)
	bestIdx := 0
	bestArea := int64(-1)
	bestDist := int64(math.MaxInt64)
	for i, m := range monitors {
		b := m.Bounds
		ow := min(r.X+rw, b.X+b.Width) - max(r.X, b.X)
		oh := min(r.Y+rh, b.Y+b.Height) - max(r.Y, b.Y)
		var area int64
		if ow > 0 && oh > 0 {
			area = int64(ow) * int64(oh)
		}
		dist := rectRectDistSq(r, b)
		if area > bestArea || (area == bestArea && dist < bestDist) {
			bestIdx, bestArea, bestDist = i, area, dist
		}
	}
	return monitors[bestIdx], true
}

func rectPointDistSq(b Rect, p Point) int64 {
	var dx, dy int64
	if p.X < b.X {
		dx = int64(b.X - p.X)
	} else if p.X >= b.X+b.Width {
		dx = int64(p.X - (b.X + b.Width - 1))
	}
	if p.Y < b.Y {
		dy = int64(b.Y - p.Y)
	} else if p.Y >= b.Y+b.Height {
		dy = int64(p.Y - (b.Y + b.Height - 1))
	}
	return dx*dx + dy*dy
}

func rectRectDistSq(a, b Rect) int64 {
	aw, ah := max(a.Width, 1), max(a.Height, 1)
	var dx, dy int64
	if a.X+aw <= b.X {
		dx = int64(b.X - (a.X + aw))
	} else if b.X+b.Width <= a.X {
		dx = int64(a.X - (b.X + b.Width))
	}
	if a.Y+ah <= b.Y {
		dy = int64(b.Y - (a.Y + ah))
	} else if b.Y+b.Height <= a.Y {
		dy = int64(a.Y - (b.Y + b.Height))
	}
	return dx*dx + dy*dy
}

// floorDiv returns a / b rounded down toward -infinity, for b > 0.
func floorDiv(a, b int) int {
	q := a / b
	if a%b != 0 && a < 0 {
		q--
	}
	return q
}

// scaledInt matches mygo's dipSpace rounding (halves up, negative-safe).
func scaledInt(v, num, den int) int {
	return floorDiv(2*v*num+den, 2*den)
}

// caretRectPlausible checks whether a raw caret rectangle looks like an actual
// text caret rather than an empty, zeroed, or full-window artifact.
func caretRectPlausible(caret Rect, hasWindow bool, window Rect) bool {
	if caret.Height <= 0 || caret.Height > 400 || caret.Width < 0 || caret.Width > 400 {
		return false
	}
	if caret.X == 0 && caret.Y == 0 && caret.Width == 0 && caret.Height == 0 {
		return false
	}
	if hasWindow && window.Width > 0 && window.Height > 0 {
		const tol = 32
		expanded := Rect{
			X:      window.X - tol,
			Y:      window.Y - tol,
			Width:  window.Width + 2*tol,
			Height: window.Height + 2*tol,
		}
		if !rectsIntersect(caret, expanded) {
			return false
		}
	}
	return true
}

// inputControlRectPlausible checks whether a focused UIA Edit/ComboBox bounding
// rectangle is a compact single-line input control suitable as a fallback anchor.
func inputControlRectPlausible(ctrl Rect, dpi int, hasWindow bool, window Rect) bool {
	if dpi <= 0 {
		dpi = 96
	}
	minW := scaledInt(20, dpi, 96)
	minH := scaledInt(10, dpi, 96)
	maxH := scaledInt(80, dpi, 96)
	if ctrl.Width < minW || ctrl.Height < minH || ctrl.Height > maxH {
		return false
	}
	if hasWindow && window.Width > 0 && window.Height > 0 {
		if ctrl.Height >= window.Height/2 {
			return false
		}
		const tol = 32
		expanded := Rect{
			X:      window.X - tol,
			Y:      window.Y - tol,
			Width:  window.Width + 2*tol,
			Height: window.Height + 2*tol,
		}
		if !rectsIntersect(ctrl, expanded) {
			return false
		}
	}
	return true
}

func rectsIntersect(a, b Rect) bool {
	aw, ah := max(a.Width, 1), max(a.Height, 1)
	bw, bh := max(b.Width, 1), max(b.Height, 1)
	return a.X < b.X+bw && a.X+aw > b.X && a.Y < b.Y+bh && a.Y+ah > b.Y
}

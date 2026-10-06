package main

import "github.com/wailsapp/wails/v3/pkg/application"

// cursorAnchorGap is the distance between the cursor and the panel edge, as in the Tauri shell.
const cursorAnchorGap = 6.0

type rect struct{ x, y, w, h float64 }

func rectOf(r application.Rect) rect {
	return rect{float64(r.X), float64(r.Y), float64(r.Width), float64(r.Height)}
}

func (r rect) contains(x, y float64) bool {
	return x >= r.x && x < r.x+r.w && y >= r.y && y < r.y+r.h
}

// axisAnchored places a panel along one axis: forward of the cursor if it fits,
// else flipped backward, else clamped onto the monitor (preferring the origin side).
func axisAnchored(cursor, origin, extent, panel float64) float64 {
	end := origin + extent
	if forward := cursor + cursorAnchorGap; forward+panel <= end {
		return forward
	}
	if backward := cursor - cursorAnchorGap - panel; backward >= origin {
		return backward
	}
	return max(end-panel, origin)
}

// centeredIn centers a panel on a monitor.
func centeredIn(m rect, width, height float64) (int, int) {
	return int(m.x + (m.w-width)/2), int(m.y + (m.h-height)/2)
}

// monitorFor picks the monitor under the cursor, falling back to the primary one.
func monitorFor(screens []*application.Screen, x, y float64, haveCursor bool) (rect, bool) {
	var primary *rect
	for _, s := range screens {
		r := rectOf(s.Bounds)
		if haveCursor && r.contains(x, y) {
			return r, true
		}
		if s.IsPrimary || primary == nil {
			p := r
			primary = &p
		}
	}
	if primary == nil {
		return rect{}, false
	}
	return *primary, true
}

// panelOrigin resolves where the panel opens. ok=false means "leave the window where it is".
func panelOrigin(position string, screens []*application.Screen, width, height float64) (int, int, bool) {
	cx, cy, haveCursor := cursorPosition()
	monitor, ok := monitorFor(screens, cx, cy, haveCursor)
	if !ok {
		return 0, 0, false
	}
	if position == "follow_cursor" && haveCursor {
		return int(axisAnchored(cx, monitor.x, monitor.w, width)), int(axisAnchored(cy, monitor.y, monitor.h, height)), true
	}
	x, y := centeredIn(monitor, width, height)
	return x, y, true
}

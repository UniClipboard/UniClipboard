package quickpanel

import "testing"

func mixedDPIDisplays() ([]PhysicalMonitor, []Display) {
	// Primary monitor: 3840x2160 at 150% (144 DPI), origin (0, 0), bottom taskbar 60px (40 DIPs).
	// Secondary monitor (right): 1920x1080 at 100% (96 DPI), physical origin (3840, 0).
	// In mygo's dipSpace (minDPI = 96):
	//   Primary origin DIP = (0, 0), size = (2560, 1440), work = (0, 0, 2560, 1400)
	//   Secondary origin DIP = (3840 * 96 / 96, 0) = (3840, 0), size = (1920, 1080), work = (3840, 0, 1920, 1040)
	// Left monitor: 1920x1080 at 100% (96 DPI), physical origin (-1920, 0).
	monitors := []PhysicalMonitor{
		{ID: 1, Bounds: Rect{X: 0, Y: 0, Width: 3840, Height: 2160}},
		{ID: 2, Bounds: Rect{X: 3840, Y: 0, Width: 1920, Height: 1080}},
		{ID: 3, Bounds: Rect{X: -1920, Y: 0, Width: 1920, Height: 1080}},
	}
	displays := []Display{
		{
			ID:          1,
			Bounds:      Rect{X: 0, Y: 0, Width: 2560, Height: 1440},
			WorkArea:    Rect{X: 0, Y: 0, Width: 2560, Height: 1400},
			ScaleFactor: 1.5,
			Primary:     true,
		},
		{
			ID:          2,
			Bounds:      Rect{X: 3840, Y: 0, Width: 1920, Height: 1080},
			WorkArea:    Rect{X: 3840, Y: 0, Width: 1920, Height: 1040},
			ScaleFactor: 1.0,
		},
		{
			ID:          3,
			Bounds:      Rect{X: -1920, Y: 0, Width: 1920, Height: 1080},
			WorkArea:    Rect{X: -1920, Y: 0, Width: 1920, Height: 1040},
			ScaleFactor: 1.0,
		},
	}
	return monitors, displays
}

func TestPhysicalPointToDIPMixedDPI(t *testing.T) {
	monitors, displays := mixedDPIDisplays()

	// Center of 4K 150% primary monitor: physical (1920, 1080) -> DIP (1280, 720).
	pt, d, ok := PhysicalPointToDIP(Point{X: 1920, Y: 1080}, monitors, displays)
	if !ok || d.ID != 1 || pt != (Point{X: 1280, Y: 720}) {
		t.Fatalf("primary point = %+v on %d (ok=%v), want (1280,720) on 1", pt, d.ID, ok)
	}

	// Point on 100% right secondary monitor: physical (4040, 200) -> DIP (4040, 200).
	pt, d, ok = PhysicalPointToDIP(Point{X: 4040, Y: 200}, monitors, displays)
	if !ok || d.ID != 2 || pt != (Point{X: 4040, Y: 200}) {
		t.Fatalf("right monitor point = %+v on %d (ok=%v), want (4040,200) on 2", pt, d.ID, ok)
	}

	// Point on left negative-coord monitor: physical (-1000, 300) -> DIP (-1000, 300).
	pt, d, ok = PhysicalPointToDIP(Point{X: -1000, Y: 300}, monitors, displays)
	if !ok || d.ID != 3 || pt != (Point{X: -1000, Y: 300}) {
		t.Fatalf("left monitor point = %+v on %d (ok=%v), want (-1000,300) on 3", pt, d.ID, ok)
	}
}

func TestResolvePlacementCenterSelectsActiveWindowThenCursorDisplay(t *testing.T) {
	monitors, displays := mixedDPIDisplays()
	const panelW, panelH = 380, 366

	// Active window on right secondary monitor -> centers on right monitor work area (3840..5760, 0..1040).
	x, y, ok := ResolvePlacement(PositionCenter, panelW, panelH, PlacementAnchor{
		HasWindow: true,
		Window:    Rect{X: 3940, Y: 100, Width: 1200, Height: 800},
		HasCursor: true,
		Cursor:    Point{X: 500, Y: 500}, // cursor on primary, but active window wins
		Monitors:  monitors,
	}, displays)
	wantX := 3840 + (1920-panelW)/2
	wantY := (1040 - panelH) / 2
	if !ok || x != wantX || y != wantY {
		t.Fatalf("center on active window display = (%d, %d, %v), want (%d, %d, true)", x, y, ok, wantX, wantY)
	}

	// No active window -> falls back to cursor display (left monitor).
	x, y, ok = ResolvePlacement(PositionCenter, panelW, panelH, PlacementAnchor{
		HasCursor: true,
		Cursor:    Point{X: -500, Y: 400},
		Monitors:  monitors,
	}, displays)
	wantX = -1920 + (1920-panelW)/2
	wantY = (1040 - panelH) / 2
	if !ok || x != wantX || y != wantY {
		t.Fatalf("center on cursor display = (%d, %d, %v), want (%d, %d, true)", x, y, ok, wantX, wantY)
	}

	// Neither window nor cursor -> falls back to primary display.
	x, y, ok = ResolvePlacement("", panelW, panelH, PlacementAnchor{Monitors: monitors}, displays)
	wantX = (2560 - panelW) / 2
	wantY = (1400 - panelH) / 2
	if !ok || x != wantX || y != wantY {
		t.Fatalf("center fallback primary = (%d, %d, %v), want (%d, %d, true)", x, y, ok, wantX, wantY)
	}
}

func TestResolvePlacementCursorOffsetsFlipsAndClamps(t *testing.T) {
	monitors, displays := mixedDPIDisplays()
	const panelW, panelH = 380, 366

	// Normal cursor on primary 150% monitor: physical (600, 300) -> DIP (400, 200).
	x, y, ok := ResolvePlacement(PositionCursor, panelW, panelH, PlacementAnchor{
		HasCursor: true,
		Cursor:    Point{X: 600, Y: 300},
		Monitors:  monitors,
	}, displays)
	if !ok || x != 400+cursorOffsetXDIP || y != 200+cursorOffsetYDIP {
		t.Fatalf("normal cursor = (%d, %d, %v), want (%d, %d, true)", x, y, ok, 400+cursorOffsetXDIP, 200+cursorOffsetYDIP)
	}

	// Cursor near bottom-right of secondary monitor (work: 3840..5760, 0..1040): should flip left and above cursor.
	x, y, ok = ResolvePlacement(PositionCursor, panelW, panelH, PlacementAnchor{
		HasCursor: true,
		Cursor:    Point{X: 5700, Y: 1000},
		Monitors:  monitors,
	}, displays)
	wantX := 5700 - cursorOffsetXDIP - panelW
	wantY := 1000 - cursorOffsetYDIP - panelH
	if !ok || x != wantX || y != wantY {
		t.Fatalf("bottom-right cursor flip = (%d, %d, %v), want (%d, %d, true)", x, y, ok, wantX, wantY)
	}

	// Cursor at very top-left (0, 0): must never land on (0, 0) because margin keeps it inside work area.
	x, y, ok = ResolvePlacement(PositionCursor, panelW, panelH, PlacementAnchor{
		HasCursor: true,
		Cursor:    Point{X: 0, Y: 0},
		Monitors:  monitors,
	}, displays)
	if !ok || x < placementMarginDIP || y < placementMarginDIP {
		t.Fatalf("top-left cursor = (%d, %d, %v), expected >= margin %d", x, y, ok, placementMarginDIP)
	}
}

func TestResolvePlacementCaretBelowFlipAboveInputBoxAndCursorFallback(t *testing.T) {
	monitors, displays := mixedDPIDisplays()
	const panelW, panelH = 380, 366

	// 1. Exact caret in upper half of primary 150% monitor: physical (900, 450, 3, 30) -> DIP (600, 300, 2, 20).
	x, y, ok := ResolvePlacement(PositionCaret, panelW, panelH, PlacementAnchor{
		Kind:      AnchorCaret,
		Rect:      Rect{X: 900, Y: 450, Width: 3, Height: 30},
		HasCursor: true,
		Cursor:    Point{X: 150, Y: 150},
		Monitors:  monitors,
	}, displays)
	if !ok || x != 600 || y != 300+20+caretGapYDIP {
		t.Fatalf("caret below = (%d, %d, %v), want (600, %d, true)", x, y, ok, 300+20+caretGapYDIP)
	}

	// 2. Exact caret near bottom of secondary monitor (y=960, h=20, work bottom=1040) -> flips ABOVE caret.
	x, y, ok = ResolvePlacement(PositionCaret, panelW, panelH, PlacementAnchor{
		Kind:      AnchorCaret,
		Rect:      Rect{X: 4100, Y: 960, Width: 2, Height: 20},
		HasCursor: true,
		Cursor:    Point{X: 4000, Y: 100},
		Monitors:  monitors,
	}, displays)
	wantY := 960 - caretGapYDIP - panelH
	if !ok || x != 4100 || y != wantY {
		t.Fatalf("caret flip above = (%d, %d, %v), want (4100, %d, true)", x, y, ok, wantY)
	}

	// 3. Compact single-line input box fallback (AnchorInputControl) anchors below the input box.
	x, y, ok = ResolvePlacement(PositionCaret, panelW, panelH, PlacementAnchor{
		Kind:      AnchorInputControl,
		Rect:      Rect{X: 4000, Y: 120, Width: 320, Height: 34},
		HasCursor: true,
		Cursor:    Point{X: 4500, Y: 500},
		Monitors:  monitors,
	}, displays)
	if !ok || x != 4000 || y != 120+34+caretGapYDIP {
		t.Fatalf("input box anchor = (%d, %d, %v), want (4000, %d, true)", x, y, ok, 120+34+caretGapYDIP)
	}

	// 4. Caret and input box both unavailable -> falls back to cursor position.
	x, y, ok = ResolvePlacement(PositionCaret, panelW, panelH, PlacementAnchor{
		Kind:      AnchorNone,
		HasCursor: true,
		Cursor:    Point{X: 4200, Y: 300},
		Monitors:  monitors,
	}, displays)
	if !ok || x != 4200+cursorOffsetXDIP || y != 300+cursorOffsetYDIP {
		t.Fatalf("caret fallback to cursor = (%d, %d, %v), want (%d, %d, true)", x, y, ok, 4200+cursorOffsetXDIP, 300+cursorOffsetYDIP)
	}
}

func TestCaretAndInputPlausibilityFilters(t *testing.T) {
	win := Rect{X: 100, Y: 100, Width: 1000, Height: 700}

	if !caretRectPlausible(Rect{X: 250, Y: 200, Width: 2, Height: 20}, true, win) {
		t.Fatal("valid caret rejected")
	}
	if caretRectPlausible(Rect{X: 0, Y: 0, Width: 0, Height: 0}, true, win) {
		t.Fatal("zero caret accepted")
	}
	if caretRectPlausible(Rect{X: 2000, Y: 200, Width: 2, Height: 20}, true, win) {
		t.Fatal("off-window caret accepted")
	}
	if caretRectPlausible(Rect{X: 120, Y: 120, Width: 900, Height: 600}, true, win) {
		t.Fatal("oversized caret accepted")
	}

	// Compact single-line search bar at 150% DPI (height 54px = 36 DIPs) should be accepted.
	if !inputControlRectPlausible(Rect{X: 200, Y: 140, Width: 300, Height: 54}, 144, true, win) {
		t.Fatal("compact single-line input rejected")
	}
	// Multiline editor (height 500px) should be rejected so caller falls back to cursor.
	if inputControlRectPlausible(Rect{X: 120, Y: 140, Width: 900, Height: 500}, 144, true, win) {
		t.Fatal("large multiline editor accepted as compact input")
	}
}

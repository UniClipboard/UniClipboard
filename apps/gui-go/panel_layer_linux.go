//go:build linux

package main

import (
	"log"
	"math"
	"sync"

	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/layershell"
	"github.com/wailsapp/wails/v3/pkg/application"
)

// The Wayland Layer Shell quick panel follows crates/uc-tauri/src/quick_panel/{layer_shell,linux}.rs. The panel is a
// GTK3 layer surface (overlay layer, keyboard exclusive while shown) with one transparent dismissal surface per output
// below it; it is placed with margins on the output under the Hyprland cursor, inside that output's work area. It is
// active only on a Wayland display whose compositor offers the protocol; every other session keeps the ordinary window.

const (
	linuxPanelWidth, linuxPanelHeight = 800.0, 560.0 // fixed Linux panel size, before the window scale
	minWindowScale, maxWindowScale    = 0.8, 1.5
	panelWorkAreaWidthCap             = 0.9 // largest share of an output's work area width
	panelWorkAreaHeightCap            = 0.8 // largest share of its height
)

var layerPanel struct {
	sync.Mutex
	active    bool
	placement *layerPlacement
	placed    [6]int // monitor, x, y, width, height of the last Place call (diagnostics for the e2e probe)
}

// layerPlacement is one output's work area, captured once per show, in the output's local logical coordinates. GDK
// already reports logical coordinates, so neither the scale nor the physical resolution belongs here.
type layerPlacement struct {
	monitor          int
	x, y, w, h       float64
	cursorX, cursorY float64
	haveCursor       bool
}

// layerLayout caps the panel at 90% of the work area's width and 80% of its height, then places it at the cursor
// (the other platforms' axis rule) or centres it.
func layerLayout(p layerPlacement, width, height float64) (x, y, w, h float64) {
	w = math.Max(math.Floor(math.Min(width, p.w*panelWorkAreaWidthCap)), 1)
	h = math.Max(math.Floor(math.Min(height, p.h*panelWorkAreaHeightCap)), 1)
	if p.haveCursor {
		return axisAnchored(p.cursorX, p.x, p.w, w), axisAnchored(p.cursorY, p.y, p.h, h), w, h
	}
	return p.x + (p.w-w)/2, p.y + (p.h-h)/2, w, h
}

func linuxPanelDimensions(windowScale float64) (float64, float64) {
	factor := 1.0
	if !math.IsNaN(windowScale) && !math.IsInf(windowScale, 0) {
		factor = math.Min(math.Max(windowScale, minWindowScale), maxWindowScale)
	}
	return linuxPanelWidth * factor, linuxPanelHeight * factor
}

// attachLayerPanel turns the freshly created, still unrealized hidden panel into a layer surface. It must run before
// the first Show (Wails realizes the window there), which holds because the panel is created with Hidden.
func attachLayerPanel(w application.Window) {
	application.InvokeSync(func() {
		supported, err := layershell.Supported()
		if err != nil {
			log.Printf("quick panel: %v; using the ordinary window", err)
			return
		}
		if !supported {
			return
		}
		handle := w.NativeWindow()
		if handle == nil {
			log.Printf("quick panel: no native window handle; using the ordinary window")
			return
		}
		if err := layershell.Attach(handle); err != nil {
			log.Printf("quick panel: Layer Shell initialization failed: %v; using the ordinary window", err)
			return
		}
		layerPanel.Lock()
		layerPanel.active = true
		layerPanel.Unlock()
		log.Printf("quick panel: Wayland Layer Shell backend initialized")
	})
}

func layerPanelActive() bool {
	layerPanel.Lock()
	defer layerPanel.Unlock()
	return layerPanel.active
}

// layerPrepareShow is Tauri's prepare_show: pick the output (cursor, else primary, else the first), capture its work
// area, place and size the panel. It reports false when the ordinary path must handle the show.
func layerPrepareShow(w application.Window, position string, windowScale float64) bool {
	if !layerPanelActive() {
		return false
	}
	var cursor *struct{ x, y float64 }
	if client := hyprlandCurrent(); client != nil {
		if c, err := client.Cursor(); err != nil {
			log.Printf("quick panel: cursor unavailable (%v); using the default output", err)
		} else {
			cursor = &struct{ x, y float64 }{c.X, c.Y}
		}
	}
	width, height := linuxPanelDimensions(windowScale)
	application.InvokeSync(func() {
		monitors := layershell.Monitors()
		if len(monitors) == 0 {
			log.Printf("quick panel: no output available")
			return
		}
		chosen := monitors[0]
		for i := len(monitors) - 1; i >= 0; i-- {
			if monitors[i].Primary {
				chosen = monitors[i]
			}
		}
		if cursor != nil {
			for _, m := range monitors {
				if cursor.x >= float64(m.X) && cursor.x < float64(m.X+m.W) && cursor.y >= float64(m.Y) && cursor.y < float64(m.Y+m.H) {
					chosen = m
					break
				}
			}
		}
		p := layerPlacement{
			monitor: chosen.Index,
			x:       float64(chosen.WorkX - chosen.X), y: float64(chosen.WorkY - chosen.Y),
			w: float64(chosen.WorkW), h: float64(chosen.WorkH),
		}
		if position == "follow_cursor" && cursor != nil {
			p.cursorX, p.cursorY, p.haveCursor = cursor.x-float64(chosen.X), cursor.y-float64(chosen.Y), true
		}
		layerPanel.Lock()
		layerPanel.placement = &p
		layerPanel.Unlock()
		x, y, lw, lh := layerLayout(p, width, height)
		placeLayerPanel(w, p.monitor, x, y, lw, lh)
	})
	return true
}

// layerShow maps the dismissal surfaces and takes the keyboard, then shows the panel; a failure releases the keyboard
// and falls through to a plain show so the user is never left without a panel.
func layerShow(w application.Window) bool {
	if !layerPanelActive() {
		return false
	}
	application.InvokeSync(func() {
		if err := layershell.Show(w.NativeWindow()); err != nil {
			log.Printf("quick panel: Layer Shell show failed: %v", err)
		}
	})
	w.Show()
	return true
}

// layerSetLayout re-applies the size for a new window scale inside the placement captured at show time.
func layerSetLayout(w application.Window, windowScale float64) bool {
	layerPanel.Lock()
	p := layerPanel.placement
	active := layerPanel.active
	layerPanel.Unlock()
	if !active {
		return false
	}
	if p == nil {
		return true
	}
	width, height := linuxPanelDimensions(windowScale)
	x, y, lw, lh := layerLayout(*p, width, height)
	application.InvokeSync(func() {
		placeLayerPanel(w, p.monitor, x, y, lw, lh)
	})
	return true
}

func placeLayerPanel(w application.Window, monitor int, x, y, width, height float64) {
	ix, iy, iw, ih := int(math.Round(x)), int(math.Round(y)), int(math.Ceil(width)), int(math.Ceil(height))
	layerPanel.Lock()
	layerPanel.placed = [6]int{monitor, ix, iy, iw, ih}
	layerPanel.Unlock()
	layershell.Place(w.NativeWindow(), monitor, ix, iy, iw, ih)
}

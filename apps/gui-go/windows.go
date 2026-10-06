package main

import (
	"context"
	"math"
	"sync/atomic"
	"time"

	"github.com/wailsapp/wails/v3/pkg/application"
	"github.com/wailsapp/wails/v3/pkg/events"
)

const (
	updaterWindowName     = "updater"
	quickPanelWindowName  = "quick-panel"
	quickPanelPrepareShow = "quick-panel://prepare-show"

	updaterWidth, updaterHeight = 520, 420

	// Geometry of the quick panel in logical pixels (non-Linux layout).
	panelBaseWidth, panelBaseHeight = 360.0, 420.0
	panelPreviewWidth, panelGap     = 360.0, 8.0
	panelWindowPadding              = 16.0
	minUIScale, maxUIScale          = 0.8, 1.5

	// Blur events this soon after showing are focus churn, not a dismissal.
	quickPanelBlurDebounce = 300 * time.Millisecond
)

// panelState tracks the two-phase quick panel show so a blur right after
// showing does not immediately hide it again.
type panelState struct {
	ready     atomic.Bool
	lastShown atomic.Int64 // unix nanoseconds
}

func clampUIScale(scale *float64) float64 {
	if scale == nil || math.IsNaN(*scale) || math.IsInf(*scale, 0) {
		return 1
	}
	return math.Min(math.Max(*scale, minUIScale), maxUIScale)
}

func panelSize(scale *float64, previewExpanded bool) (int, int) {
	s := clampUIScale(scale)
	width := panelBaseWidth * s
	if previewExpanded {
		width = (panelBaseWidth + panelGap + panelPreviewWidth) * s
	}
	return int(math.Round(width + 2*panelWindowPadding)), int(math.Round(panelBaseHeight*s + 2*panelWindowPadding))
}

// openUpdater creates the decorated updater window, or focuses the existing one.
func (h *HostService) openUpdater(dev bool) {
	if w, ok := h.app.Window.GetByName(updaterWindowName); ok {
		w.UnMinimise()
		w.Show()
		w.Focus()
		return
	}
	url := "/updater.html"
	if dev {
		url += "?dev=1"
	}
	w := h.app.Window.NewWithOptions(application.WebviewWindowOptions{
		Name: updaterWindowName, Title: "Software Update", URL: url,
		Width: updaterWidth, Height: updaterHeight, DisableResize: true,
	})
	w.Center()
}

// preCreateQuickPanel builds the hidden, frameless quick panel at startup so
// showing it later never has to create a window.
func (h *HostService) preCreateQuickPanel() {
	width, height := panelSize(nil, false)
	w := h.app.Window.NewWithOptions(application.WebviewWindowOptions{
		Name: quickPanelWindowName, Title: "Quick Panel", URL: "/quick-panel.html",
		Width: width, Height: height, Hidden: true, Frameless: true, DisableResize: true, AlwaysOnTop: true,
		BackgroundType: application.BackgroundTypeTransparent,
		Mac:            application.MacWindow{DisableShadow: true},
	})
	w.RegisterHook(events.Common.WindowClosing, func(e *application.WindowEvent) {
		if h.quitting.Load() {
			return
		}
		e.Cancel()
		w.Hide()
	})
	w.OnWindowEvent(events.Common.WindowLostFocus, func(*application.WindowEvent) {
		if time.Since(time.Unix(0, h.panel.lastShown.Load())) > quickPanelBlurDebounce {
			w.Hide()
		}
	})
}

// showQuickPanel is phase one: size and center the window, then ask the
// frontend to clear stale state. The frontend finishes with finalize_quick_panel_show.
func (h *HostService) showQuickPanel() {
	w, ok := h.app.Window.GetByName(quickPanelWindowName)
	if !ok {
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	prefs, err := h.loadQuickPanelSettings(ctx)
	cancel()
	if err == nil && !prefs.Enabled {
		return // the user turned the quick panel off
	}
	width, height := panelSize(nil, false)
	w.SetSize(width, height)
	if x, y, ok := panelOrigin(prefs.Position, h.app.Screen.GetAll(), float64(width), float64(height)); ok {
		w.SetPosition(x, y)
	} else {
		w.Center()
	}
	h.panel.lastShown.Store(time.Now().UnixNano())
	h.emit(quickPanelPrepareShow, nil)
}

func (h *HostService) dismissQuickPanel() {
	if w, ok := h.app.Window.GetByName(quickPanelWindowName); ok {
		w.Hide()
	}
}

func init() {
	register(map[string]commandFunc{
		"open_updater_window": func(_ context.Context, h *HostService, _ commandArgs) (any, error) {
			h.openUpdater(false)
			return nil, nil
		},
		"dev_open_updater_window": func(_ context.Context, h *HostService, _ commandArgs) (any, error) {
			h.openUpdater(true)
			return nil, nil
		},
		"show_content_unlock": func(_ context.Context, h *HostService, _ commandArgs) (any, error) {
			if w, ok := h.app.Window.GetByName("main"); ok {
				w.Show()
				w.Focus()
			}
			return nil, nil
		},
		"dismiss_quick_panel": func(_ context.Context, h *HostService, _ commandArgs) (any, error) {
			h.dismissQuickPanel()
			return nil, nil
		},
		"set_quick_panel_layout": func(_ context.Context, h *HostService, args commandArgs) (any, error) {
			var scale *float64
			var expanded bool
			if err := args.decode("scale", &scale); err != nil {
				return nil, err
			}
			if err := args.decode("previewExpanded", &expanded); err != nil {
				return nil, err
			}
			if w, ok := h.app.Window.GetByName(quickPanelWindowName); ok {
				width, height := panelSize(scale, expanded)
				w.SetSize(width, height)
			}
			return nil, nil
		},
		"finalize_quick_panel_show": func(_ context.Context, h *HostService, _ commandArgs) (any, error) {
			if w, ok := h.app.Window.GetByName(quickPanelWindowName); ok {
				w.Show()
				w.Focus()
			}
			return nil, nil
		},
		"mark_quick_panel_ready": func(_ context.Context, h *HostService, _ commandArgs) (any, error) {
			h.panel.ready.Store(true)
			return nil, nil
		},
		// No global shortcut backend exists in this host yet.
		"get_quick_panel_double_tap_availability": func(context.Context, *HostService, commandArgs) (any, error) {
			return "unsupported_display_session", nil
		},
		"quick_panel_uses_compositor_shortcuts": func(context.Context, *HostService, commandArgs) (any, error) { return false, nil },
		"resolve_quick_panel_expand_side":       func(context.Context, *HostService, commandArgs) (any, error) { return "right", nil },
	})
}

package main

import (
	"context"
	"errors"
	"log"
	"math"
	"runtime"
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

	// Linux panel (X11 window and Wayland Layer Shell alike): fixed size, the content zoom never changes it; only the
	// window scale does (crates/uc-tauri/src/quick_panel/mod.rs: LINUX_PANEL_WIDTH/HEIGHT, resized_panel_dimensions).
	linuxPanelWidth, linuxPanelHeight = 800.0, 560.0
	minWindowScale, maxWindowScale    = 0.8, 1.5

	// Blur events this soon after showing are focus churn, not a dismissal.
	quickPanelBlurDebounce = 300 * time.Millisecond
	// A blur is only a dismissal when focus is still gone this long after it: AttachThreadInput detaches, IME
	// popups and WebView focus shuffles raise short spurious blurs (same value as the Tauri shell).
	quickPanelBlurVerify = 100 * time.Millisecond
)

// errPreviousAppUnsupported is what the paste commands report where no implementation exists (never a silent success).
var errPreviousAppUnsupported = errors.New("Paste to previous app is not yet supported on this platform")

// panelState tracks the two-phase quick panel show so a blur right after
// showing does not immediately hide it again.
type panelState struct {
	toggle    toggleState
	lastShown atomic.Int64 // unix nanoseconds
}

func clampUIScale(scale *float64) float64 {
	if scale == nil || math.IsNaN(*scale) || math.IsInf(*scale, 0) {
		return 1
	}
	return math.Min(math.Max(*scale, minUIScale), maxUIScale)
}

// windowScaleOrOne is the Linux window scale as Tauri receives it: absent or not finite means 1.
func windowScaleOrOne(scale *float64) float64 {
	if scale == nil || math.IsNaN(*scale) || math.IsInf(*scale, 0) {
		return 1
	}
	return *scale
}

// linuxPanelDimensions is the Linux panel size in logical pixels for a window scale (not finite means 1).
func linuxPanelDimensions(windowScale float64) (float64, float64) {
	factor := 1.0
	if !math.IsNaN(windowScale) && !math.IsInf(windowScale, 0) {
		factor = math.Min(math.Max(windowScale, minWindowScale), maxWindowScale)
	}
	return linuxPanelWidth * factor, linuxPanelHeight * factor
}

// panelSize is the quick panel window size. Linux uses the fixed 800x560 contract (content zoom and the preview
// pane do not change it); the other platforms size the window around the floating cards.
func panelSize(scale *float64, previewExpanded bool, windowScale float64) (int, int) {
	if runtime.GOOS == "linux" {
		width, height := linuxPanelDimensions(windowScale)
		return int(math.Round(width)), int(math.Round(height))
	}
	s := clampUIScale(scale)
	width := panelBaseWidth * s
	if previewExpanded {
		width = (panelBaseWidth + panelGap + panelPreviewWidth) * s
	}
	return int(math.Round(width + 2*panelWindowPadding)), int(math.Round(panelBaseHeight*s + 2*panelWindowPadding))
}

// setPanelSize sizes the quick panel. The panel is created non-resizable, and GTK honours neither a default size nor a
// resize request that is smaller than the current size of a non-resizable window (it pins min = max = the current size, which
// the X11 WM_NORMAL_HINTS show). Wails beta.28 SetSize is gtk_window_set_default_size, so on Linux the window is made
// resizable around the request and locked again, which is Tauri's gtk_window_resize on the same non-resizable window.
func setPanelSize(w application.Window, width, height int) {
	if runtime.GOOS != "linux" {
		w.SetSize(width, height)
		return
	}
	w.SetResizable(true)
	w.SetSize(width, height)
	w.SetResizable(false)
}

// openUpdater creates the decorated updater window, or focuses the existing one.
func (h *HostService) openUpdater(dev bool) {
	if w, ok := h.app.Window.GetByName(updaterWindowName); ok {
		w.UnMinimise()
		w.Show()
		focusWindow(w)
		return
	}
	url := "/updater.html"
	if dev {
		url += "?dev=1"
	}
	w := h.app.Window.NewWithOptions(quietOptions(application.WebviewWindowOptions{
		Name: updaterWindowName, Title: "Software Update", URL: url,
		Width: updaterWidth, Height: updaterHeight, DisableResize: true,
	}))
	centerWindow(w)
}

// preCreateQuickPanel builds the hidden, frameless quick panel at startup so
// showing it later never has to create a window.
func (h *HostService) preCreateQuickPanel() {
	width, height := panelSize(nil, false, 1)
	w := h.app.Window.NewWithOptions(quietOptions(application.WebviewWindowOptions{
		Name: quickPanelWindowName, Title: "Quick Panel", URL: "/quick-panel.html",
		Width: width, Height: height, Hidden: true, Frameless: true, DisableResize: true, AlwaysOnTop: true,
		BackgroundType: application.BackgroundTypeTransparent,
		Mac:            application.MacWindow{DisableShadow: true},
	}))
	attachLayerPanel(w) // Wayland Layer Shell: must happen while the hidden window is still unrealized
	w.RegisterHook(events.Common.WindowClosing, func(e *application.WindowEvent) {
		if h.quitting.Load() {
			return
		}
		e.Cancel()
		w.Hide()
	})
	w.OnWindowEvent(events.Common.WindowLostFocus, func(*application.WindowEvent) {
		if time.Since(time.Unix(0, h.panel.lastShown.Load())) <= quickPanelBlurDebounce {
			return
		}
		time.AfterFunc(quickPanelBlurVerify, func() {
			if !w.IsFocused() {
				w.Hide()
			}
		})
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
	if !layerPrepareShow(w, prefs.Position, 1) {
		width, height := panelSize(nil, false, 1)
		setPanelSize(w, width, height)
		if x, y, ok := panelOrigin(prefs.Position, h.app.Screen.GetAll(), float64(width), float64(height)); ok {
			moveWindow(w, x, y)
		} else {
			centerWindow(w)
		}
	}
	if previousAppInputSupported {
		// Before the panel takes the focus: what is foreground now is where the paste must go.
		_ = runOnMainThread(func() error { rememberPreviousForeground(w); return nil })
	}
	h.panel.lastShown.Store(time.Now().UnixNano())
	h.emit(quickPanelPrepareShow, nil)
}

// dismissQuickPanel hides the panel and hands the keyboard focus back to the window that had it.
func (h *HostService) dismissQuickPanel() {
	if w, ok := h.app.Window.GetByName(quickPanelWindowName); ok {
		w.Hide()
	}
	if previousAppInputSupported && dismissRestoresPrevious {
		if err := runOnMainThread(restorePreviousForeground); err != nil {
			log.Printf("quick panel dismiss could not restore the previous foreground window: %v", err)
		}
	}
}

// toggleQuickPanel shows the hidden panel and dismisses the visible one.
func (h *HostService) toggleQuickPanel() {
	if w, ok := h.app.Window.GetByName(quickPanelWindowName); ok && w.IsVisible() {
		h.dismissQuickPanel()
		return
	}
	h.showQuickPanel()
}

// requestPanelToggle is the entry for the global shortcut and for `--quick-panel` launches.
func (h *HostService) requestPanelToggle() {
	if h.helper != nil {
		return // the native helper owns its panel and shortcut; a request has nothing to toggle here
	}
	if h.panel.toggle.request() {
		h.toggleQuickPanel()
	}
}

// panelFocus gives the shown panel keyboard focus. On Windows the foreground lock blocks a plain Focus, so the
// panel claims the foreground the way restorePreviousForeground returns it.
func panelFocus(w application.Window) {
	if previousAppInputSupported && !quiet() {
		_ = runOnMainThread(func() error { forceForegroundWindow(w); return nil })
		return
	}
	focusWindow(w)
}

// pasteIntoPreviousApp hides the panel, restores the previous window and sends the paste keystroke (or types
// the text). A failure shows the panel again, like the Tauri shell, so the selection is not lost.
func (h *HostService) pasteIntoPreviousApp(send func() error) error {
	if !previousAppInputSupported {
		return errPreviousAppUnsupported
	}
	if w, ok := h.app.Window.GetByName(quickPanelWindowName); ok {
		w.Hide()
	}
	err := runOnMainThread(restorePreviousForeground)
	if err == nil {
		err = send()
	}
	if err != nil {
		h.showQuickPanel()
	}
	return err
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
			h.showMainWindow()
			return nil, nil
		},
		"dismiss_quick_panel": func(_ context.Context, h *HostService, _ commandArgs) (any, error) {
			h.dismissQuickPanel()
			return nil, nil
		},
		"set_quick_panel_layout": func(_ context.Context, h *HostService, args commandArgs) (any, error) {
			var scale *float64
			var expanded bool
			var windowScale *float64
			if err := args.decode("scale", &scale); err != nil {
				return nil, err
			}
			if err := args.decode("previewExpanded", &expanded); err != nil {
				return nil, err
			}
			if _, present := args["windowScale"]; present {
				if err := args.decode("windowScale", &windowScale); err != nil {
					return nil, err
				}
			}
			if w, ok := h.app.Window.GetByName(quickPanelWindowName); ok {
				if !layerSetLayout(w, windowScaleOrOne(windowScale)) {
					width, height := panelSize(scale, expanded, windowScaleOrOne(windowScale))
					setPanelSize(w, width, height)
				}
			}
			return nil, nil
		},
		"finalize_quick_panel_show": func(_ context.Context, h *HostService, _ commandArgs) (any, error) {
			if w, ok := h.app.Window.GetByName(quickPanelWindowName); ok {
				if layerShow(w) {
					return nil, nil // the exclusive keyboard mode already gives it the focus
				}
				w.Show()
				panelFocus(w)
			}
			return nil, nil
		},
		"mark_quick_panel_ready": func(_ context.Context, h *HostService, _ commandArgs) (any, error) {
			if h.panel.toggle.markReady() {
				h.toggleQuickPanel()
			}
			return nil, nil
		},
		"quick_panel_uses_compositor_shortcuts": func(context.Context, *HostService, commandArgs) (any, error) { return usesCompositorShortcuts(), nil },
		"resolve_quick_panel_expand_side":       func(context.Context, *HostService, commandArgs) (any, error) { return "right", nil },
	})
}

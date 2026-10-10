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
	updaterWindowName    = "updater"
	quickPanelWindowName = "quick-panel"

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

// setPanelSize sizes the quick panel. On Linux the panel is fixed-size: the size is pinned by geometry hints (Wails
// SetMinSize/SetMaxSize, min = max = the target) instead of the GTK non-resizable flag, because GTK pins a non-resizable
// window to its creation size and can then only grow it (gtk_window_resize, which Wails SetSize calls on gtk3), so a
// smaller window scale never took effect (17c9). The hints keep the window manager from resizing it. Releasing the minimum
// first and the maximum second lets the window move to a smaller or a larger target.
func setPanelSize(w application.Window, width, height int) {
	if runtime.GOOS != "linux" {
		w.SetSize(width, height)
		return
	}
	w.SetMinSize(0, 0)
	w.SetMaxSize(width, height)
	w.SetSize(width, height)
	w.SetMinSize(width, height)
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
	suppressKeyboardMenu(w)
	centerWindow(w)
}

// preCreateQuickPanel builds the hidden, frameless quick panel at startup so
// showing it later never has to create a window.
func (h *HostService) preCreateQuickPanel() {
	width, height := panelSize(nil, false, 1)
	options := application.WebviewWindowOptions{
		Name: quickPanelWindowName, Title: "Quick Panel", URL: "/quick-panel.html",
		Width: width, Height: height, Hidden: true, Frameless: true, DisableResize: true, AlwaysOnTop: true,
		BackgroundType: application.BackgroundTypeTransparent,
		Mac:            application.MacWindow{DisableShadow: true},
	}
	w := h.app.Window.NewWithOptions(quietOptions(options))
	suppressKeyboardMenu(w)
	attachLayerPanel(w) // Wayland Layer Shell: must happen while the hidden window is still unrealized
	if runtime.GOOS == "linux" && !layerPanelActive() {
		// The ordinary X11/XWayland window: fixed size by geometry hints instead of the GTK flag (see setPanelSize). The
		// Layer Shell surface keeps its own sizing: a minimum hint above a capped surface size breaks small outputs (17c9 F7).
		w.SetResizable(true)
		setPanelSize(w, width, height)
	}
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

// OpenUpdaterWindow opens the software update window, or focuses it when it is already open.
//
//uc:errors none
//uc:os all=real
func (h *HostService) OpenUpdaterWindow() {
	h.openUpdater(false)
}

// DevOpenUpdaterWindow opens the update window in preview mode (`?dev=1`), which shows a sample release. It is a
// development aid of the settings page; the window itself decides what the flag shows.
//
//uc:errors none
//uc:os all=real
func (h *HostService) DevOpenUpdaterWindow() {
	h.openUpdater(true)
}

// DismissQuickPanel hides the quick panel and gives the focus back to the application that had it.
//
//uc:errors none
//uc:os all=real
func (h *HostService) DismissQuickPanel() {
	h.dismissQuickPanel()
}

// SetQuickPanelLayout sizes the quick panel for the page's content scale, the preview pane and (Linux) the window
// scale. A null scale means 1.
//
//uc:errors none
//uc:os all=real
func (h *HostService) SetQuickPanelLayout(scale *float64, previewExpanded bool, windowScale *float64) {
	if w, ok := h.app.Window.GetByName(quickPanelWindowName); ok {
		if !layerSetLayout(w, windowScaleOrOne(windowScale)) {
			width, height := panelSize(scale, previewExpanded, windowScaleOrOne(windowScale))
			setPanelSize(w, width, height)
		}
	}
}

// FinalizeQuickPanelShow is the second phase of showing the panel, after the page cleared its stale state.
//
//uc:errors none
//uc:os all=real
func (h *HostService) FinalizeQuickPanelShow() {
	if w, ok := h.app.Window.GetByName(quickPanelWindowName); ok {
		if layerShow(w) {
			return // the exclusive keyboard mode already gives it the focus
		}
		w.Show()
		panelFocus(w)
	}
}

// MarkQuickPanelReady tells the host the panel page finished loading; a toggle requested earlier then takes effect.
//
//uc:errors none
//uc:os all=real
func (h *HostService) MarkQuickPanelReady() {
	if h.panel.toggle.markReady() {
		h.toggleQuickPanel()
	}
}

// QuickPanelUsesCompositorShortcuts reports whether the shortcut is bound by the Wayland compositor instead of the app.
//
//uc:errors none
//uc:os all=real
func (h *HostService) QuickPanelUsesCompositorShortcuts() bool {
	return usesCompositorShortcuts()
}

// ResolveQuickPanelExpandSide tells which side the inline preview opens toward. This host always opens to the right.
//
//uc:errors none
//uc:os all=noop
func (h *HostService) ResolveQuickPanelExpandSide(scale *float64) QuickPanelExpandSide {
	return ExpandSideRight
}

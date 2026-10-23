package main

import (
	"context"
	"log"
	"math"
	"runtime"
	"time"

	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/hostapi"
	"github.com/wailsapp/wails/v3/pkg/application"
)

// Native shell, appearance and lifecycle commands.

// SetWindowZoom changes the calling WebView's native zoom, including scales below 100%.
//
//uc:errors command ValidationError NotFound InternalError
//uc:os all=real
//uc:adapter @/host/webview
func (h *HostService) SetWindowZoom(ctx context.Context, scale float64) error {
	if math.IsNaN(scale) || math.IsInf(scale, 0) || scale <= 0 {
		return hostapi.New(hostapi.CodeValidationError, "zoom must be a finite positive number")
	}
	w, ok := ctx.Value(application.WindowKey).(application.Window)
	if !ok || w == nil {
		return hostapi.New(hostapi.CodeNotFound, "calling window is unavailable")
	}
	var err error
	application.InvokeSync(func() { err = setWindowZoom(w, scale) })
	if err != nil {
		return hostapi.Internal(err)
	}
	return nil
}

func hostPlatform() string {
	if runtime.GOOS == "darwin" {
		return "macos"
	}
	return runtime.GOOS
}

// GetDesktopTheme reports the current Omarchy palette and device-local preference.
//
//uc:errors none
//uc:os darwin=noop windows=noop linux=real
func (h *HostService) GetDesktopTheme() DesktopThemeSnapshot {
	if uiChild {
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		var snapshot DesktopThemeSnapshot
		if err := h.ownerCall(ctx, "desktop-theme", nil, &snapshot); err != nil {
			log.Printf("desktop theme request failed: %v", err)
		}
		return snapshot
	}
	return h.desktopTheme.snapshot()
}

// SetFollowOmarchyTheme saves the preference before publishing it to all windows.
//
//uc:errors command InternalError
//uc:os darwin=noop windows=noop linux=real
func (h *HostService) SetFollowOmarchyTheme(enabled bool) (DesktopThemeSnapshot, error) {
	if uiChild {
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		var snapshot DesktopThemeSnapshot
		err := h.ownerCall(ctx, "desktop-theme-follow", enabled, &snapshot)
		return snapshot, err
	}
	snapshot, err := h.desktopTheme.setEnabled(enabled)
	if err != nil {
		return snapshot, hostapi.Internal(err)
	}
	h.emit(desktopThemeChangedEvent, snapshot)
	return snapshot, nil
}

// TakePendingNavigation returns, once, the route a tray or second launch asked the page to open.
//
//uc:errors none
//uc:os all=real
func (h *HostService) TakePendingNavigation() *string {
	h.navMu.Lock()
	defer h.navMu.Unlock()
	if h.pendingNavigation == "" {
		return nil
	}
	route := h.pendingNavigation
	h.pendingNavigation = ""
	return &route
}

// MainWindowPresentationReady is the page's handshake that the main window finished its first paint. The Go host
// shows the window itself and has a single window generation, so there is nothing to reconcile: it is inert.
//
//uc:errors none
//uc:os all=noop
func (h *HostService) MainWindowPresentationReady(generation string) {}

// MarkMainWindowReady is the page's handshake that the main window is ready to be shown. Inert for the same reason.
//
//uc:errors none
//uc:os all=noop
func (h *HostService) MarkMainWindowReady(generation string) {}

// SetTrafficLightPosition would place the macOS window buttons. Inert: Wails draws them with the hidden-inset title
// bar at a fixed place, and the page's offset is not applied.
//
//uc:errors none
//uc:os darwin=unsupported windows=noop linux=noop
func (h *HostService) SetTrafficLightPosition(offsetX *float64, offsetY *float64) {}

// GetInstallKind tells how this copy was installed, so the page can route package-managed copies to their package
// manager instead of the in-app updater.
//
//uc:errors none
//uc:os all=real
func (h *HostService) GetInstallKind() InstallKind {
	return installKind()
}

func installKind() InstallKind {
	if runtime.GOOS == "darwin" {
		return InstallKindMacOS
	}
	return platformInstallKind()
}

// SetWindowDecorations applies the page's frame preference (custom controls drawn by the page, or the system
// frame) to the main window. macOS keeps its hidden-inset title bar either way.
//
//uc:errors none
//uc:os all=real darwin=noop
//uc:adapter @/host/window
func (h *HostService) SetWindowDecorations(decorations bool) {
	if runtime.GOOS == "darwin" {
		return
	}
	if w, ok := h.app.Window.GetByName("main"); ok {
		w.SetFrameless(!decorations)
	}
}

// RestartApp restarts the daemon and then the GUI.
//
//uc:errors none
//uc:os all=real
func (h *HostService) RestartApp() {
	go h.fullRestart()
}

// RestartDaemon replaces the daemon process; the page reconnects itself.
//
//uc:errors command InternalError
//uc:os all=real
func (h *HostService) RestartDaemon(ctx context.Context) error {
	if uiChild {
		return h.ownerCall(ctx, "restart-daemon", nil, nil)
	}
	h.announceDaemonStop()
	err := restartDaemon()
	// Reconnect the host and the windows to the new daemon or, after a failure, to whatever daemon runs.
	h.resumeAfterStop(false)
	if err != nil {
		return hostapi.Internal(err)
	}
	return nil
}

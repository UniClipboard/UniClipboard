package main

import (
	"context"
	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/hostapi"
	"runtime"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
)

// Native shell, appearance and lifecycle commands.

func hostPlatform() string {
	if runtime.GOOS == "darwin" {
		return "macos"
	}
	return runtime.GOOS
}

// unavailableTheme is the desktop theme of a host without an Omarchy theme source (every host today).
func unavailableTheme() DesktopThemeSnapshot { return DesktopThemeSnapshot{} }

// GetDesktopTheme reports the desktop theme. Only a Linux host with an Omarchy theme source could report one; this
// host has none on any platform, so it always answers "unavailable" and `desktop-theme://changed` is never sent.
//
//uc:errors none
//uc:os darwin=noop windows=noop linux=unsupported
func (h *HostService) GetDesktopTheme() DesktopThemeSnapshot {
	return unavailableTheme()
}

// SetFollowOmarchyTheme would switch the page to the Omarchy theme. Unsupported: with no theme source the request
// changes nothing and the answer stays "unavailable".
//
//uc:errors none
//uc:os darwin=noop windows=noop linux=unsupported
func (h *HostService) SetFollowOmarchyTheme(enabled bool) DesktopThemeSnapshot {
	return unavailableTheme()
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

// SetTrayLanguage updates the tray menu labels to the UI language. It does not persist anything.
//
//uc:errors none
//uc:os all=real
func (h *HostService) SetTrayLanguage(language string) {
	defer e2eInvoke("set_tray_language")() // e2e builds record when this command enters and leaves; a no-op otherwise
	h.tray.setLanguage(language)
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
	h.announceDaemonStop()
	if err := restartDaemon(); err != nil {
		return hostapi.Internal(err)
	}
	// The new process has its own connection file and token: re-read them, then tell the pages to reconnect.
	client, err := daemonclient.FromEnv()
	if err != nil {
		return hostapi.Internal(err)
	}
	h.client.Store(client)
	h.emit(daemonConnectionChanged, nil)
	return nil
}

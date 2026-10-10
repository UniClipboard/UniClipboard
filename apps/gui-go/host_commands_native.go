package main

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"runtime"
	"sync"

	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/hostapi"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
)

// Native shell, appearance and lifecycle commands.

func hostPlatform() string {
	if runtime.GOOS == "darwin" {
		return "macos"
	}
	return runtime.GOOS
}

// visualEffects keeps the per-process preference the React frontend reads. It is
// session-only until the host owns a persisted preference store.
type visualEffects struct {
	mu           sync.Mutex
	sessionID    string
	revision     int
	mode         EffectsMode
	systemMotion SystemMotion
}

func newVisualEffects() *visualEffects {
	id := make([]byte, 8)
	_, _ = rand.Read(id)
	return &visualEffects{sessionID: hex.EncodeToString(id), revision: 1, mode: EffectsModeAuto, systemMotion: SystemMotionUnknown}
}

// snapshot reads the state; callers hold v.mu.
func (v *visualEffects) snapshot() EffectsSnapshot {
	reduce := v.systemMotion == SystemMotionReduce
	reason := EffectsReasonPlatformDefault
	switch {
	case v.mode != EffectsModeAuto:
		reason = EffectsReasonManual
	case reduce:
		reason = EffectsReasonSystem
	}
	return EffectsSnapshot{
		SessionID: v.sessionID, Revision: v.revision, Mode: v.mode,
		AutoForSession: AutoResultEffects, SystemMotion: v.systemMotion,
		ReduceMotion: reduce, LowEffects: v.mode == EffectsModeSmooth || reduce,
		Reason: reason, Persistence: EffectsPersistenceSessionOnly,
	}
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

// GetVisualEffects returns the visual effects state of this session.
//
//uc:errors none
//uc:os all=real
func (h *HostService) GetVisualEffects() EffectsSnapshot {
	h.effects.mu.Lock()
	defer h.effects.mu.Unlock()
	return h.effects.snapshot()
}

// SetVisualEffectsMode sets the visual effects preference and tells every window.
//
//uc:errors command ValidationError
//uc:os all=real
func (h *HostService) SetVisualEffectsMode(mode EffectsMode) (EffectsSnapshot, error) {
	switch mode {
	case EffectsModeAuto, EffectsModeEffects, EffectsModeSmooth:
	default:
		return EffectsSnapshot{}, hostapi.New(hostapi.CodeValidationError, "unknown visual effects mode")
	}
	h.effects.mu.Lock()
	h.effects.mode = mode
	h.effects.revision++
	snap := h.effects.snapshot()
	h.effects.mu.Unlock()
	h.emit(visualEffectsChangedEvent, snap)
	return snap, nil
}

// ReportVisualEffectsEnvironment records the system reduce-motion preference the page observed.
//
//uc:errors command ValidationError
//uc:os all=real
func (h *HostService) ReportVisualEffectsEnvironment(sessionID string, systemMotion SystemMotion) (EffectsSnapshot, error) {
	switch systemMotion {
	case SystemMotionReduce, SystemMotionAllow, SystemMotionUnknown:
	default:
		return EffectsSnapshot{}, hostapi.New(hostapi.CodeValidationError, "unknown system motion")
	}
	h.effects.mu.Lock()
	defer h.effects.mu.Unlock()
	if systemMotion != h.effects.systemMotion {
		h.effects.systemMotion = systemMotion
		h.effects.revision++
	}
	return h.effects.snapshot(), nil
}

// BeginVisualEffectsSample asks for permission to measure one sample. This host does not sample frames, so no
// permit is ever granted (always nil).
//
//uc:errors none
//uc:os all=noop
func (h *HostService) BeginVisualEffectsSample(sessionID string) *SamplePermit {
	return nil
}

// ReportVisualEffectsSample accepts a frame-timing sample. It is read and dropped: this host does not sample.
//
//uc:errors none
//uc:os all=noop
func (h *HostService) ReportVisualEffectsSample(sample EffectsSample) EffectsSnapshot {
	h.effects.mu.Lock()
	defer h.effects.mu.Unlock()
	return h.effects.snapshot()
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

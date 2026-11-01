package main

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"log"
	"net/http"
	"runtime"
	"slices"
	"strings"
	"time"

	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/hostapi"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/globalshortcut"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/quickpanelhelper"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/settings"
)

// loadQuickPanelSettings reads the persisted quick-panel preferences; the daemon
// settings are the single source of truth, so nothing is cached in the host.
func (h *HostService) loadQuickPanelSettings(ctx context.Context) (settings.QuickPanel, error) {
	view, err := settings.Read(ctx, h.daemon())
	return view.QuickPanel, err
}

func (h *HostService) patchQuickPanel(ctx context.Context, patch map[string]any) error {
	body := map[string]any{"quickPanel": patch}
	if err := h.daemon().Enveloped(ctx, daemonclient.Request{Method: http.MethodPut, Path: "/settings", JSON: body}, nil); err != nil {
		return hostapi.Internal(err)
	}
	return nil
}

// SetQuickPanelEnabled turns the quick panel and its global shortcut on or off.
//
//uc:errors command InternalError Conflict
//uc:os all=real
func (h *HostService) SetQuickPanelEnabled(ctx context.Context, enabled bool) error {
	if uiChild {
		return h.ownerCall(ctx, "quick-enabled", enabled, nil)
	}
	ctx, cancel := commandContext(ctx, "set_quick_panel_enabled")
	defer cancel()
	current, err := h.loadQuickPanelSettings(ctx)
	if err != nil {
		return hostapi.Internal(err)
	}
	if h.lightweight.Load() {
		return h.patchQuickPanel(ctx, map[string]any{"enabled": enabled})
	}
	if current.Enabled == enabled {
		return nil
	}
	if h.helper == nil {
		return h.setWebViewPanelEnabled(ctx, enabled)
	}
	h.shortcutsMu.Lock()
	defer h.shortcutsMu.Unlock()
	view, err := settings.Read(ctx, h.daemon())
	if err != nil {
		return hostapi.Internal(err)
	}
	if err := h.patchQuickPanel(ctx, map[string]any{"enabled": enabled}); err != nil {
		return err
	}
	// The helper process is the executor: a started helper is told the binding on launch (Relaunched).
	h.setHelperEnabled(enabled, panelShortcutTarget(enabled, view.KeyboardShortcuts))
	h.appliedPanel.enabled = enabled
	return nil
}

// SetQuickPanelPosition saves where the quick panel appears.
//
//uc:errors command ValidationError InternalError
//uc:os all=real
func (h *HostService) SetQuickPanelPosition(ctx context.Context, position QuickPanelPosition) error {
	ctx, cancel := commandContext(ctx, "set_quick_panel_position")
	defer cancel()
	if position != QuickPanelPositionCenter && position != QuickPanelPositionFollowCursor {
		return hostapi.New(hostapi.CodeValidationError, "invalid argument position")
	}
	return h.patchQuickPanel(ctx, map[string]any{"position": position})
}

// GetQuickPanelAccessibilityPermission checks the system grant without prompting or depending on panel enablement.
//
//uc:errors none
//uc:os darwin=real windows=unsupported linux=unsupported
func (h *HostService) GetQuickPanelAccessibilityPermission() bool {
	return accessibilityTrusted()
}

// RefreshQuickPanelTriggers asks the running native executor to reapply authoritative settings.
// A successful request is not a receipt for helper permission or listener initialization.
//
//uc:errors command InternalError
//uc:os all=real
func (h *HostService) RefreshQuickPanelTriggers(ctx context.Context) error {
	if uiChild {
		return h.ownerCall(ctx, "quick-refresh-triggers", nil, nil)
	}
	ctx, cancel := commandContext(ctx, "refresh_quick_panel_triggers")
	defer cancel()
	h.shortcutsMu.Lock()
	defer h.shortcutsMu.Unlock()
	if h.helper == nil || h.lightweight.Load() {
		return nil
	}
	current, err := h.loadQuickPanelSettings(ctx)
	if err != nil {
		return hostapi.Internal(err)
	}
	if current.Enabled {
		// ReloadSettings neither starts a stopped helper nor changes persisted preferences.
		h.helper.ReloadSettings()
	}
	return nil
}

// GetQuickPanelDoubleTapAvailability tells whether the modifier double tap trigger can work in this session.
//
//uc:errors none
//uc:os all=real
func (h *HostService) GetQuickPanelDoubleTapAvailability() ModifierDoubleTapAvailability {
	if uiChild {
		var result ModifierDoubleTapAvailability
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		if err := h.ownerCall(ctx, "quick-availability", nil, &result); err != nil {
			log.Printf("modifier availability request failed: %v", err)
			return DoubleTapUnsupportedDisplaySession
		}
		return result
	}
	switch {
	case h.helper == nil && modifierDoubleTapSupported():
		return DoubleTapSupported // the WebView panel's own monitor (Windows)
	case h.helper == nil || !helperSupportsDoubleTap():
		return DoubleTapUnsupportedDisplaySession
	case runtime.GOOS == "darwin" && !accessibilityTrusted():
		return DoubleTapAccessibilityPermissionNeeded
	}
	return DoubleTapSupported
}

// helperSupportsDoubleTap reports whether the native helper implements the modifier double tap on this platform
// (macOS through Core Graphics, Windows through GetAsyncKeyState; Linux has no such reader).
func helperSupportsDoubleTap() bool { return runtime.GOOS == "darwin" || runtime.GOOS == "windows" }

// PasteToPreviousApp hides the panel, returns to the previously focused application and pastes. It rejects with a
// plain string (hostapi.TextError); the panel is shown again on failure so the selection is not lost.
//
//uc:errors text
//uc:os darwin=unsupported windows=real linux=real
func (h *HostService) PasteToPreviousApp() error {
	return asTextError(h.pasteIntoPreviousApp(simulatePaste))
}

// TypeFilePathsToPreviousApp types file paths, one per line, into the previously focused application.
//
//uc:errors text
//uc:os darwin=unsupported windows=real linux=real
func (h *HostService) TypeFilePathsToPreviousApp(request FilePathInputRequest) error {
	if len(request.FilePaths) == 0 {
		return hostapi.TextError("No valid file paths were provided")
	}
	for _, path := range request.FilePaths {
		if path == "" {
			return hostapi.TextError("No valid file paths were provided")
		}
	}
	return asTextError(h.pasteIntoPreviousApp(func() error { return simulateTextInput(strings.Join(request.FilePaths, "\n")) }))
}

// UpdateKeyboardShortcuts merges a shortcut patch into the settings and re-binds the global shortcut. A null value
// clears the shortcut; any other value is one accelerator string or a list of alternatives (opaque JSON here, see
// UpdateKeyboardShortcutsResult).
//
//uc:errors command InternalError Conflict
//uc:os all=real
func (h *HostService) UpdateKeyboardShortcuts(ctx context.Context, shortcuts map[string]json.RawMessage) (UpdateKeyboardShortcutsResult, error) {
	if uiChild {
		var result UpdateKeyboardShortcutsResult
		err := h.ownerCall(ctx, "quick-shortcuts", shortcuts, &result)
		return result, err
	}
	ctx, cancel := commandContext(ctx, "update_keyboard_shortcuts")
	defer cancel()
	return h.updateKeyboardShortcuts(ctx, shortcuts)
}

// SetQuickPanelDoubleTapModifier sets the modifier whose double tap opens the quick panel.
//
//uc:errors command ValidationError Conflict InternalError
//uc:os all=real
func (h *HostService) SetQuickPanelDoubleTapModifier(ctx context.Context, modifier QuickPanelDoubleTapModifier) error {
	if uiChild {
		return h.ownerCall(ctx, "quick-modifier", modifier, nil)
	}
	ctx, cancel := commandContext(ctx, "set_quick_panel_double_tap_modifier")
	defer cancel()
	switch modifier {
	case DoubleTapModifierDisabled, DoubleTapModifierAlt, DoubleTapModifierControl, DoubleTapModifierMeta:
	default:
		return hostapi.New(hostapi.CodeValidationError, "invalid argument modifier")
	}
	if h.lightweight.Load() {
		return h.patchQuickPanel(ctx, map[string]any{"doubleTapModifier": modifier})
	}
	if h.helper == nil {
		return h.setWebViewModifier(ctx, string(modifier))
	}
	// The native helper owns the trigger and implements it on macOS and Windows: accepting the setting elsewhere
	// would promise a trigger that never fires.
	if modifier != DoubleTapModifierDisabled && !helperSupportsDoubleTap() {
		return hostapi.New(hostapi.CodeConflict, "modifier double-tap is not available with the native quick panel on this platform yet")
	}
	h.shortcutsMu.Lock()
	defer h.shortcutsMu.Unlock()
	current, err := h.loadQuickPanelSettings(ctx)
	if err != nil {
		return hostapi.Internal(err)
	}
	if current.DoubleTapModifier != string(modifier) {
		if err := h.patchQuickPanel(ctx, map[string]any{"doubleTapModifier": modifier}); err != nil {
			return err
		}
		// The running helper reads its settings again and applies the trigger; no restart.
		h.helper.ReloadSettings()
	}
	h.appliedPanel.modifier = string(modifier)
	return nil
}

// quickPanelShortcutKey is the setting id of the quick panel's global shortcut.
const quickPanelShortcutKey = "global.toggleQuickPanel"

// updateKeyboardShortcuts merges a shortcut patch into the daemon settings. The executor that holds the quick panel's
// global shortcut (the native helper or the host's Wails binder) takes the new binding before it is saved, so a
// refused binding is reported and nothing is saved (ADR-013 section 10).
func (h *HostService) updateKeyboardShortcuts(ctx context.Context, patch map[string]json.RawMessage) (UpdateKeyboardShortcutsResult, error) {
	h.shortcutsMu.Lock()
	defer h.shortcutsMu.Unlock()
	view, err := settings.Read(ctx, h.daemon())
	if err != nil {
		return UpdateKeyboardShortcutsResult{}, hostapi.Internal(err)
	}
	next := map[string]json.RawMessage{}
	for id, value := range view.KeyboardShortcuts {
		next[id] = value
	}
	for id, value := range patch {
		if bytes.Equal(bytes.TrimSpace(value), []byte("null")) {
			delete(next, id)
		} else {
			next[id] = value
		}
	}
	var persistErr error
	persist := func() error {
		body := map[string]any{"keyboardShortcuts": map[string]any{"shortcuts": patch}}
		persistErr = h.daemon().Enveloped(ctx, daemonclient.Request{Method: http.MethodPut, Path: "/settings", JSON: body}, nil)
		return persistErr
	}
	if h.lightweight.Load() {
		if err := persist(); err != nil {
			return UpdateKeyboardShortcutsResult{}, hostapi.Internal(err)
		}
		return UpdateKeyboardShortcutsResult{KeyboardShortcuts: next}, nil
	}
	if err := h.panelShortcut.Save(panelShortcutTarget(view.QuickPanel.Enabled, next), persist); err != nil {
		return UpdateKeyboardShortcutsResult{}, shortcutSaveError(err, persistErr)
	}
	return UpdateKeyboardShortcutsResult{KeyboardShortcuts: next}, nil
}

// shortcutSaveError maps a failed globalshortcut.Applier.Save: the save's own error is internal, an executor that
// refused the binding is a conflict, and an executor that is not running or did not answer is internal.
func shortcutSaveError(err, persistErr error) error {
	switch {
	case persistErr != nil && errors.Is(err, persistErr):
		if _, ok := err.(hostapi.CommandError); ok {
			return err
		}
		return hostapi.Internal(err)
	case errors.Is(err, globalshortcut.ErrUnknown), errors.Is(err, quickpanelhelper.ErrNotRunning):
		return hostapi.Internal(err)
	}
	return hostapi.New(hostapi.CodeConflict, err.Error())
}

func sameShortcutSet(a, b []string) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}

// setWebViewPanelEnabled enables or disables the WebView quick panel: the OS shortcut moves first (a conflict is
// refused before anything is saved) and is undone when saving the setting fails.
func (h *HostService) setWebViewPanelEnabled(ctx context.Context, enabled bool) error {
	h.shortcutsMu.Lock()
	defer h.shortcutsMu.Unlock()
	view, err := settings.Read(ctx, h.daemon())
	if err != nil {
		return hostapi.Internal(err)
	}
	var persistErr error
	persist := func() error {
		// The modifier trigger follows the enabled switch like the shortcut does: watched only while the panel is on.
		previousModifier := h.modifierMonitor().Current()
		if err := h.modifierMonitor().Set(desiredLiveModifier(enabled, modifierDoubleTapSupported(), view.QuickPanel.DoubleTapModifier)); err != nil {
			persistErr = hostapi.New(hostapi.CodeConflict, err.Error())
			return persistErr
		}
		if err := h.patchQuickPanel(ctx, map[string]any{"enabled": enabled}); err != nil {
			if rollback := h.modifierMonitor().Set(previousModifier); rollback != nil {
				log.Printf("failed to roll the modifier double-tap trigger back after the settings save failed: %v", rollback)
			}
			persistErr = err
			return err
		}
		return nil
	}
	if err := h.panelShortcut.Save(panelShortcutTarget(enabled, view.KeyboardShortcuts), persist); err != nil {
		return shortcutSaveError(err, persistErr)
	}
	h.appliedPanel.enabled = enabled
	h.panel.toggle.setEnabled(enabled)
	if !enabled {
		h.dismissQuickPanel()
	}
	return nil
}

// setWebViewModifier is `set_quick_panel_double_tap_modifier` for the WebView panel: the monitor changes first (an
// unsupported session is refused before anything is saved) and is put back when saving fails (Tauri order).
func (h *HostService) setWebViewModifier(ctx context.Context, modifier string) error {
	h.shortcutsMu.Lock()
	defer h.shortcutsMu.Unlock()
	current, err := h.loadQuickPanelSettings(ctx)
	if err != nil {
		return hostapi.Internal(err)
	}
	if current.Enabled && modifier != "disabled" && !modifierDoubleTapSupported() {
		return hostapi.New(hostapi.CodeConflict, "modifier double-tap is not available with this quick panel on this platform yet")
	}
	previous := h.modifierMonitor().Current()
	if err := h.modifierMonitor().Set(desiredLiveModifier(current.Enabled, modifierDoubleTapSupported(), modifier)); err != nil {
		return hostapi.New(hostapi.CodeConflict, err.Error())
	}
	if current.DoubleTapModifier == modifier {
		h.appliedPanel.modifier = modifier
		return nil
	}
	if err := h.patchQuickPanel(ctx, map[string]any{"doubleTapModifier": modifier}); err != nil {
		if rollback := h.modifierMonitor().Set(previous); rollback != nil {
			log.Printf("failed to roll the modifier double-tap trigger back after the settings save failed: %v", rollback)
		}
		return err
	}
	h.appliedPanel.modifier = modifier
	return nil
}

// modifierMonitor returns the WebView panel's modifier trigger monitor, created on first use. A trigger toggles the
// panel exactly like the global shortcut does.
func (h *HostService) modifierMonitor() *modifierMonitor {
	h.modifierOnce.Do(func() {
		h.modifier = newModifierMonitor(modifierKeyStateFactory(), func() {
			e2eModifierTriggered()
			h.requestPanelToggle()
		})
	})
	return h.modifier
}

// panelEnabled projects the stored preference onto the current residency mode.
// Lightweight hosts construct executors once but keep their resources inactive.
func (h *HostService) panelEnabled(configured bool) bool {
	return configured && !h.lightweight.Load()
}

// initPanelShortcuts registers the quick panel's global shortcut at startup when the WebView panel is in use (the
// native helper is told its binding when it launches). A shortcut the OS refuses is recorded in the status, not
// fatal: the panel stays reachable from the tray and the user can pick another shortcut in settings.
func (h *HostService) initPanelShortcuts() {
	if h.helper != nil {
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	enabled := true // the setting's default when it cannot be read
	view, err := settings.Read(ctx, h.daemon())
	if err == nil {
		enabled = view.QuickPanel.Enabled
	} else {
		log.Printf("quick panel shortcut: settings unreadable, using the defaults: %v", err)
	}
	enabled = h.panelEnabled(enabled)
	h.shortcutsMu.Lock()
	h.panelShortcut.Init(panelShortcutTarget(enabled, view.KeyboardShortcuts))
	if err := h.modifierMonitor().Set(desiredLiveModifier(enabled, modifierDoubleTapSupported(), view.QuickPanel.DoubleTapModifier)); err != nil {
		log.Printf("quick panel modifier double-tap not started: %v", err)
	}
	h.appliedPanel = appliedPanelSettings{known: err == nil, enabled: enabled, modifier: view.QuickPanel.DoubleTapModifier}
	h.shortcutsMu.Unlock()
	pending := h.panel.toggle.configure(enabled)
	if h.ui != nil {
		// The resident can dispatch now; the child separately waits for its page readiness.
		pending = h.panel.toggle.markReady() || pending
	}
	if pending {
		h.toggleQuickPanel()
	}
}

// appliedPanelSettings is what the host last applied of `quickPanel.enabled` and `quickPanel.doubleTapModifier`, so
// a settings read that brings the same values does nothing (ADR-013 section 10).
type appliedPanelSettings struct {
	known    bool
	enabled  bool
	modifier string
}

// reconcilePanel applies an external change of the quick panel's shortcut, enabled switch or double-tap modifier.
// It runs after every settings read; the host's own saves record what they applied, so their events change nothing.
// When anything differs the settings are read again under the lock, so a read older than the host's own save cannot
// bring an old value back.
func (h *HostService) reconcilePanel(view settings.View) {
	if h.panelShortcut == nil {
		return
	}
	h.shortcutsMu.Lock()
	defer h.shortcutsMu.Unlock()
	view.QuickPanel.Enabled = h.panelEnabled(view.QuickPanel.Enabled)
	status := h.panelShortcut.Status()
	target := panelShortcutTarget(view.QuickPanel.Enabled, view.KeyboardShortcuts)
	applied := h.appliedPanel
	if applied.known && applied.enabled == view.QuickPanel.Enabled && applied.modifier == view.QuickPanel.DoubleTapModifier &&
		slices.Equal(target, status.Desired) && status.State != globalshortcut.StateUnknown {
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	fresh, err := settings.Read(ctx, h.daemon())
	cancel()
	if err != nil {
		log.Printf("quick panel: settings changed but could not be read again: %v", err)
		return
	}
	qp := fresh.QuickPanel
	qp.Enabled = h.panelEnabled(qp.Enabled)
	target = panelShortcutTarget(qp.Enabled, fresh.KeyboardShortcuts)
	// appliedPanel.enabled is what startup actually applied, also when its read failed (the default), so it is
	// compared as is: an unchanged switch must reach Reconcile, or a helper started with the default binding would
	// never be told the saved one (F109). The modifier read at a failed startup is unknown and is applied once.
	enabledChanged := applied.enabled != qp.Enabled
	modifierChanged := !applied.known || applied.modifier != qp.DoubleTapModifier
	h.appliedPanel = appliedPanelSettings{known: true, enabled: qp.Enabled, modifier: qp.DoubleTapModifier}
	if h.helper != nil {
		if enabledChanged {
			h.setHelperEnabled(qp.Enabled, target) // a launched helper reads its settings and is told its binding
			return
		}
		h.panelShortcut.Reconcile(target, func() ([]string, error) { return target, nil })
		if modifierChanged {
			h.helper.ReloadSettings()
		}
		return
	}
	h.panelShortcut.Reconcile(target, func() ([]string, error) { return target, nil })
	if enabledChanged || modifierChanged {
		if err := h.modifierMonitor().Set(desiredLiveModifier(qp.Enabled, modifierDoubleTapSupported(), qp.DoubleTapModifier)); err != nil {
			log.Printf("quick panel modifier double-tap not applied: %v", err)
		}
	}
	if enabledChanged {
		h.panel.toggle.setEnabled(qp.Enabled)
		if !qp.Enabled {
			h.dismissQuickPanel()
		}
	}
}

// GetQuickPanelShortcutStatus reports whether the saved quick panel shortcut is the one the OS holds: active, saved
// but not active (with the binding still held and the OS error), unknown, or inactive (ADR-013 section 10). The host
// emits quick-panel://shortcut-status on every change.
//
//uc:errors none
//uc:os all=real
func (h *HostService) GetQuickPanelShortcutStatus() QuickPanelShortcutStatus {
	if uiChild {
		var result QuickPanelShortcutStatus
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		if err := h.ownerCall(ctx, "quick-status", nil, &result); err != nil {
			log.Printf("shortcut status request failed: %v", err)
			return toShortcutStatus(globalshortcut.Status{State: globalshortcut.StateUnknown})
		}
		return result
	}
	if h.panelShortcut == nil {
		return toShortcutStatus(globalshortcut.Status{State: globalshortcut.StateInactive})
	}
	return toShortcutStatus(h.panelShortcut.Status())
}

func toShortcutStatus(s globalshortcut.Status) QuickPanelShortcutStatus {
	desired, active := s.Desired, s.Active
	if desired == nil {
		desired = []string{}
	}
	if active == nil {
		active = []string{}
	}
	return QuickPanelShortcutStatus{ShortcutID: quickPanelShortcutKey, State: QuickPanelShortcutState(s.State), Desired: desired, Active: active, Error: s.Error}
}

// asTextError turns a failure into the plain-string rejection of the quick panel commands.
func asTextError(err error) error {
	if err == nil {
		return nil
	}
	return hostapi.TextError(err.Error())
}

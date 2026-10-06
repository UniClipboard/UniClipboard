package main

import (
	"bytes"
	"context"
	"encoding/json"
	"log"
	"net/http"
	"runtime"
	"strings"
	"time"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
)

type quickPanelSettings struct {
	Enabled           bool   `json:"enabled"`
	Position          string `json:"position"`
	DoubleTapModifier string `json:"doubleTapModifier"`
}

// loadQuickPanelSettings reads the persisted quick-panel preferences; the daemon
// settings are the single source of truth, so nothing is cached in the host.
func (h *HostService) loadQuickPanelSettings(ctx context.Context) (quickPanelSettings, error) {
	var settings struct {
		QuickPanel quickPanelSettings `json:"quickPanel"`
	}
	err := h.client.Get(ctx, "/settings", &settings)
	return settings.QuickPanel, err
}

func (h *HostService) patchQuickPanel(ctx context.Context, patch map[string]any) error {
	body := map[string]any{"quickPanel": patch}
	if err := h.client.Enveloped(ctx, daemonclient.Request{Method: http.MethodPut, Path: "/settings", JSON: body}, nil); err != nil {
		return internalError(err)
	}
	return nil
}

func init() {
	register(map[string]commandFunc{
		"set_quick_panel_enabled": func(ctx context.Context, h *HostService, args commandArgs) (any, error) {
			var enabled bool
			if err := args.decode("enabled", &enabled); err != nil {
				return nil, err
			}
			current, err := h.loadQuickPanelSettings(ctx)
			if err != nil {
				return nil, internalError(err)
			}
			if current.Enabled == enabled {
				return nil, nil
			}
			if h.helper == nil {
				return nil, h.setWebViewPanelEnabled(ctx, enabled)
			}
			if err := h.patchQuickPanel(ctx, map[string]any{"enabled": enabled}); err != nil {
				return nil, err
			}
			h.panelEnabledChanged(enabled)
			return nil, nil
		},
		"set_quick_panel_position": func(ctx context.Context, h *HostService, args commandArgs) (any, error) {
			var position string
			if err := args.decode("position", &position); err != nil {
				return nil, err
			}
			if position != "center" && position != "follow_cursor" {
				return nil, commandError{Code: "ValidationError", Message: "invalid argument position"}
			}
			return nil, h.patchQuickPanel(ctx, map[string]any{"position": position})
		},
		"get_quick_panel_double_tap_availability": func(_ context.Context, h *HostService, _ commandArgs) (any, error) {
			switch {
			case h.helper == nil && modifierDoubleTapSupported():
				return "supported", nil // the WebView panel's own monitor (Windows)
			case h.helper == nil || runtime.GOOS != "darwin":
				return "unsupported_display_session", nil
			case !accessibilityTrusted():
				return "accessibility_permission_required", nil
			}
			return "supported", nil
		},
		"paste_to_previous_app": func(_ context.Context, h *HostService, _ commandArgs) (any, error) {
			return nil, asStringError(h.pasteIntoPreviousApp(simulatePaste))
		},
		"type_file_paths_to_previous_app": func(_ context.Context, h *HostService, args commandArgs) (any, error) {
			var request struct {
				FilePaths []string `json:"filePaths"`
			}
			if err := args.decode("request", &request); err != nil {
				return nil, err
			}
			if len(request.FilePaths) == 0 {
				return nil, stringError("No valid file paths were provided")
			}
			for _, path := range request.FilePaths {
				if path == "" {
					return nil, stringError("No valid file paths were provided")
				}
			}
			return nil, asStringError(h.pasteIntoPreviousApp(func() error { return simulateTextInput(strings.Join(request.FilePaths, "\n")) }))
		},
		"update_keyboard_shortcuts": func(ctx context.Context, h *HostService, args commandArgs) (any, error) {
			var patch map[string]json.RawMessage // a null value clears the shortcut
			if err := args.decode("shortcuts", &patch); err != nil {
				return nil, err
			}
			return h.updateKeyboardShortcuts(ctx, patch)
		},
		"set_quick_panel_double_tap_modifier": func(ctx context.Context, h *HostService, args commandArgs) (any, error) {
			var modifier string
			if err := args.decode("modifier", &modifier); err != nil {
				return nil, err
			}
			switch modifier {
			case "disabled", "alt", "control", "meta":
			default:
				return nil, commandError{Code: "ValidationError", Message: "invalid argument modifier"}
			}
			if h.helper == nil {
				return nil, h.setWebViewModifier(ctx, modifier)
			}
			// The native helper owns the trigger and implements it on macOS only (Tauri `supports_double_tap`):
			// accepting the setting elsewhere would promise a trigger that never fires.
			if modifier != "disabled" && runtime.GOOS != "darwin" {
				return nil, commandError{Code: "Conflict", Message: "modifier double-tap is not available with the native quick panel on this platform yet"}
			}
			// The helper reads the trigger at startup: persist, then restart it.
			current, err := h.loadQuickPanelSettings(ctx)
			if err != nil {
				return nil, internalError(err)
			}
			if current.DoubleTapModifier != modifier {
				if err := h.patchQuickPanel(ctx, map[string]any{"doubleTapModifier": modifier}); err != nil {
					return nil, err
				}
				h.restartPanelHelper()
			}
			return nil, nil
		},
	})
}

// quickPanelShortcutKey is the setting id of the quick panel's global shortcut.
const quickPanelShortcutKey = "global.toggleQuickPanel"

// updateKeyboardShortcuts merges a shortcut patch into the daemon settings. The native helper
// registers its global shortcut at startup, so it is restarted when that shortcut changed.
func (h *HostService) updateKeyboardShortcuts(ctx context.Context, patch map[string]json.RawMessage) (any, error) {
	h.shortcutsMu.Lock()
	defer h.shortcutsMu.Unlock()
	var settings struct {
		KeyboardShortcuts map[string]json.RawMessage `json:"keyboardShortcuts"`
		QuickPanel        quickPanelSettings         `json:"quickPanel"`
	}
	if err := h.client.Get(ctx, "/settings", &settings); err != nil {
		return nil, internalError(err)
	}
	next := map[string]json.RawMessage{}
	for id, value := range settings.KeyboardShortcuts {
		next[id] = value
	}
	for id, value := range patch {
		if bytes.Equal(bytes.TrimSpace(value), []byte("null")) {
			delete(next, id)
		} else {
			next[id] = value
		}
	}
	// Without the native helper the host owns the OS binding: move it first so a conflicting shortcut is refused
	// before anything is saved, and undo it when the save fails so the OS and the persisted setting agree.
	var previousOS []string
	osChanged := false
	if h.helper == nil {
		previousOS = h.osShortcuts
		target := panelShortcutTarget(settings.QuickPanel.Enabled, next)
		if !sameShortcutSet(previousOS, target) {
			if err := h.applyOSShortcuts(target); err != nil {
				return nil, err
			}
			osChanged = true
		}
	}
	body := map[string]any{"keyboardShortcuts": map[string]any{"shortcuts": patch}}
	if err := h.client.Enveloped(ctx, daemonclient.Request{Method: http.MethodPut, Path: "/settings", JSON: body}, nil); err != nil {
		if osChanged {
			if rollbackErr := h.applyOSShortcuts(previousOS); rollbackErr != nil {
				log.Printf("failed to roll the global shortcut back after the settings save failed: %v", rollbackErr)
			}
		}
		return nil, internalError(err)
	}
	if !sameShortcut(settings.KeyboardShortcuts[quickPanelShortcutKey], next[quickPanelShortcutKey]) {
		h.restartPanelHelper()
	}
	return map[string]any{"keyboardShortcuts": next}, nil
}

func sameShortcut(a, b json.RawMessage) bool {
	var ca, cb bytes.Buffer
	if json.Compact(&ca, a) != nil || json.Compact(&cb, b) != nil {
		return bytes.Equal(a, b)
	}
	return bytes.Equal(ca.Bytes(), cb.Bytes())
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
	var settings struct {
		KeyboardShortcuts map[string]json.RawMessage `json:"keyboardShortcuts"`
		QuickPanel        quickPanelSettings         `json:"quickPanel"`
	}
	if err := h.client.Get(ctx, "/settings", &settings); err != nil {
		return internalError(err)
	}
	previous := h.osShortcuts
	if err := h.applyOSShortcuts(panelShortcutTarget(enabled, settings.KeyboardShortcuts)); err != nil {
		return err
	}
	// The modifier trigger follows the enabled switch like the shortcut does: watched only while the panel is on.
	previousModifier := h.modifierMonitor().Current()
	if err := h.modifierMonitor().Set(desiredLiveModifier(enabled, modifierDoubleTapSupported(), settings.QuickPanel.DoubleTapModifier)); err != nil {
		h.rollbackPanelOS(previous, previousModifier)
		return commandError{Code: "Conflict", Message: err.Error()}
	}
	if err := h.patchQuickPanel(ctx, map[string]any{"enabled": enabled}); err != nil {
		h.rollbackPanelOS(previous, previousModifier)
		return err
	}
	h.panel.toggle.setEnabled(enabled)
	if !enabled {
		h.dismissQuickPanel()
	}
	return nil
}

// rollbackPanelOS puts the global shortcut and the modifier trigger back after a failed change, so the OS state
// and the persisted settings agree. Failures are only logged: the original error is the one to report.
func (h *HostService) rollbackPanelOS(shortcuts []string, modifier string) {
	if err := h.applyOSShortcuts(shortcuts); err != nil {
		log.Printf("failed to roll the global shortcut back after the settings save failed: %v", err)
	}
	if err := h.modifierMonitor().Set(modifier); err != nil {
		log.Printf("failed to roll the modifier double-tap trigger back after the settings save failed: %v", err)
	}
}

// setWebViewModifier is `set_quick_panel_double_tap_modifier` for the WebView panel: the monitor changes first (an
// unsupported session is refused before anything is saved) and is put back when saving fails (Tauri order).
func (h *HostService) setWebViewModifier(ctx context.Context, modifier string) error {
	h.shortcutsMu.Lock()
	defer h.shortcutsMu.Unlock()
	current, err := h.loadQuickPanelSettings(ctx)
	if err != nil {
		return internalError(err)
	}
	if current.Enabled && modifier != "disabled" && !modifierDoubleTapSupported() {
		return commandError{Code: "Conflict", Message: "modifier double-tap is not available with this quick panel on this platform yet"}
	}
	previous := h.modifierMonitor().Current()
	if err := h.modifierMonitor().Set(desiredLiveModifier(current.Enabled, modifierDoubleTapSupported(), modifier)); err != nil {
		return commandError{Code: "Conflict", Message: err.Error()}
	}
	if current.DoubleTapModifier == modifier {
		return nil
	}
	if err := h.patchQuickPanel(ctx, map[string]any{"doubleTapModifier": modifier}); err != nil {
		if rollback := h.modifierMonitor().Set(previous); rollback != nil {
			log.Printf("failed to roll the modifier double-tap trigger back after the settings save failed: %v", rollback)
		}
		return err
	}
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

// initPanelShortcuts registers the quick panel's global shortcut at startup when the WebView panel is in use (the
// native helper registers its own). A shortcut the OS refuses is logged, not fatal: the panel stays reachable from
// the tray and the user can pick another shortcut in settings.
func (h *HostService) initPanelShortcuts() {
	if h.helper != nil {
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	var settings struct {
		KeyboardShortcuts map[string]json.RawMessage `json:"keyboardShortcuts"`
		QuickPanel        quickPanelSettings         `json:"quickPanel"`
	}
	enabled := true // the setting's default when it cannot be read
	if err := h.client.Get(ctx, "/settings", &settings); err == nil {
		enabled = settings.QuickPanel.Enabled
	} else {
		log.Printf("quick panel shortcut: settings unreadable, using the defaults: %v", err)
	}
	h.shortcutsMu.Lock()
	if err := h.applyOSShortcuts(panelShortcutTarget(enabled, settings.KeyboardShortcuts)); err != nil {
		log.Printf("quick panel shortcut not registered: %v", err)
	}
	if err := h.modifierMonitor().Set(desiredLiveModifier(enabled, modifierDoubleTapSupported(), settings.QuickPanel.DoubleTapModifier)); err != nil {
		log.Printf("quick panel modifier double-tap not started: %v", err)
	}
	h.shortcutsMu.Unlock()
	if h.panel.toggle.configure(enabled) {
		h.toggleQuickPanel()
	}
}

func asStringError(err error) error {
	if err == nil {
		return nil
	}
	return stringError(err.Error())
}

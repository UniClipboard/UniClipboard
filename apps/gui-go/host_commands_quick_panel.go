package main

import (
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"runtime"

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
			case h.helper == nil || runtime.GOOS != "darwin":
				return "unsupported_display_session", nil
			case !accessibilityTrusted():
				return "accessibility_permission_required", nil
			}
			return "supported", nil
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
			// The modifier double-tap trigger lives in the native helper (macOS); the WebView panel
			// has none, and accepting the setting would promise a trigger that never fires.
			if modifier != "disabled" && (h.helper == nil || runtime.GOOS != "darwin") {
				return nil, commandError{Code: "Conflict", Message: "modifier double-tap is not available with this quick panel on this platform yet"}
			}
			current, err := h.loadQuickPanelSettings(ctx)
			if err != nil {
				return nil, internalError(err)
			}
			if current.DoubleTapModifier != modifier {
				if err := h.patchQuickPanel(ctx, map[string]any{"doubleTapModifier": modifier}); err != nil {
					return nil, err
				}
				// The helper reads the trigger at startup: restart it so the new value takes effect.
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
	body := map[string]any{"keyboardShortcuts": map[string]any{"shortcuts": patch}}
	if err := h.client.Enveloped(ctx, daemonclient.Request{Method: http.MethodPut, Path: "/settings", JSON: body}, nil); err != nil {
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

package main

import (
	"context"
	"net/http"

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
			if !enabled {
				h.dismissQuickPanel()
			}
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
			if modifier != "disabled" {
				// Accepting the setting would promise a trigger that never fires: this host has no
				// modifier double-tap monitor yet (see get_quick_panel_double_tap_availability).
				return nil, commandError{Code: "Conflict", Message: "modifier double-tap is not available in this host yet"}
			}
			return nil, h.patchQuickPanel(ctx, map[string]any{"doubleTapModifier": modifier})
		},
	})
}

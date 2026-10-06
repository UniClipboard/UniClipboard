package main

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"runtime"
	"sync"
)

const visualEffectsChangedEvent = "visual-effects://changed"

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
	mode         string
	systemMotion string
}

func newVisualEffects() *visualEffects {
	id := make([]byte, 8)
	_, _ = rand.Read(id)
	return &visualEffects{sessionID: hex.EncodeToString(id), revision: 1, mode: "auto", systemMotion: "unknown"}
}

func (v *visualEffects) snapshot() map[string]any {
	reduce := v.systemMotion == "reduce"
	low := v.mode == "smooth" || reduce
	reason := "platform_default"
	switch {
	case v.mode != "auto":
		reason = "manual"
	case reduce:
		reason = "system"
	}
	return map[string]any{
		"sessionId": v.sessionID, "revision": v.revision, "mode": v.mode,
		"autoForSession": "effects", "nextAuto": nil, "systemMotion": v.systemMotion,
		"reduceMotion": reduce, "lowEffects": low, "reason": reason, "persistence": "session_only",
	}
}

func init() {
	register(map[string]commandFunc{
		// The Omarchy theme source exists only on Linux hosts that ship it.
		"get_desktop_theme": func(context.Context, *HostService, commandArgs) (any, error) {
			return map[string]any{"revision": 0, "followOmarchyTheme": false, "omarchyAvailable": false, "theme": nil, "windowCornerRadius": nil}, nil
		},
		"set_follow_omarchy_theme": func(context.Context, *HostService, commandArgs) (any, error) {
			return map[string]any{"revision": 0, "followOmarchyTheme": false, "omarchyAvailable": false, "theme": nil, "windowCornerRadius": nil}, nil
		},
		"get_visual_effects": func(_ context.Context, h *HostService, _ commandArgs) (any, error) {
			h.effects.mu.Lock()
			defer h.effects.mu.Unlock()
			return h.effects.snapshot(), nil
		},
		"set_visual_effects_mode": func(_ context.Context, h *HostService, args commandArgs) (any, error) {
			var mode string
			if err := args.decode("mode", &mode); err != nil {
				return nil, err
			}
			h.effects.mu.Lock()
			h.effects.mode = mode
			h.effects.revision++
			snap := h.effects.snapshot()
			h.effects.mu.Unlock()
			h.emit(visualEffectsChangedEvent, snap)
			return snap, nil
		},
		"report_visual_effects_environment": func(_ context.Context, h *HostService, args commandArgs) (any, error) {
			var motion string
			if err := args.decode("systemMotion", &motion); err != nil {
				return nil, err
			}
			h.effects.mu.Lock()
			defer h.effects.mu.Unlock()
			if motion != h.effects.systemMotion {
				h.effects.systemMotion = motion
				h.effects.revision++
			}
			return h.effects.snapshot(), nil
		},
		// Frame sampling is not performed by this host, so no permit is ever granted.
		"begin_visual_effects_sample": func(context.Context, *HostService, commandArgs) (any, error) { return nil, nil },
		"report_visual_effects_sample": func(_ context.Context, h *HostService, _ commandArgs) (any, error) {
			h.effects.mu.Lock()
			defer h.effects.mu.Unlock()
			return h.effects.snapshot(), nil
		},
		"take_pending_navigation": func(_ context.Context, h *HostService, _ commandArgs) (any, error) {
			return h.takePendingNavigation(), nil
		},
		"main_window_presentation_ready": func(context.Context, *HostService, commandArgs) (any, error) { return nil, nil },
		"mark_main_window_ready":         func(context.Context, *HostService, commandArgs) (any, error) { return nil, nil },
		"set_traffic_light_position":     func(context.Context, *HostService, commandArgs) (any, error) { return nil, nil },
		"get_install_kind":               func(context.Context, *HostService, commandArgs) (any, error) { return installKind(), nil },
	})
}

func installKind() string {
	if runtime.GOOS == "darwin" {
		return "macos"
	}
	return "unknown"
}

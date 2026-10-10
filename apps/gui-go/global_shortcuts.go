package main

import (
	"encoding/json"
	"log"
	"runtime"
	"strings"
	"sync"
	"time"

	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/hostapi"
	"github.com/wailsapp/wails/v3/pkg/application"
)

// The WebView quick panel (every platform without the native panel helper) is toggled by a global shortcut
// registered through Wails' app.GlobalShortcut (Win32 RegisterHotKey on Windows). Wails owns the OS binding,
// conflict detection and the release on exit; this file only carries the Uni rules that sit on top of it and
// that the Tauri shell implements in crates/uc-desktop/src/shortcuts.rs and
// crates/uc-tauri/src/quick_panel/shortcut_registry.rs: the settings value and its default, the physical-key
// normalization, two-step chords, and the unregister-old, register-new, roll-back-on-failure transition.

// chordWindow is how long the second step of a chord may follow the first (matches the frontend's CHORD_WINDOW_MS).
const chordWindow = time.Second

// maxChordSegments is the longest chord the runtime supports; longer bindings are cut to their first two steps.
const maxChordSegments = 2

func defaultQuickPanelShortcut() string {
	if override, ok := shortcutDefaultOverride(); ok {
		return override
	}
	if runtime.GOOS == "darwin" {
		return "super+ctrl+v"
	}
	return "ctrl+alt+v"
}

// normalizeShortcutKeys turns a frontend binding ("meta+ctrl+v", "mod+shift+v", or a two-step chord separated by a
// space) into the physical-key form: meta becomes super, the abstract mod/cmd/command key becomes the platform's
// primary modifier, everything else is lower-cased.
func normalizeShortcutKeys(binding string) string {
	var segments []string
	for _, segment := range strings.Fields(binding) {
		if len(segments) == maxChordSegments {
			break
		}
		parts := strings.Split(segment, "+")
		for i, part := range parts {
			switch lower := strings.ToLower(strings.TrimSpace(part)); lower {
			case "meta", "super":
				parts[i] = "super"
			case "mod", "cmd", "command":
				if runtime.GOOS == "darwin" {
					parts[i] = "super"
				} else {
					parts[i] = "ctrl"
				}
			case "control":
				parts[i] = "ctrl"
			default:
				parts[i] = lower
			}
		}
		segments = append(segments, strings.Join(parts, "+"))
	}
	return strings.Join(segments, " ")
}

// resolveQuickPanelShortcuts reads the quick panel binding out of the daemon's keyboardShortcuts map; the value is
// a string or a list of strings. An unset or empty value falls back to the platform default, so the result is
// never empty.
func resolveQuickPanelShortcuts(shortcuts map[string]json.RawMessage) []string {
	var values []string
	if raw, ok := shortcuts[quickPanelShortcutKey]; ok {
		var one string
		var many []string
		if json.Unmarshal(raw, &one) == nil {
			values = []string{one}
		} else if json.Unmarshal(raw, &many) == nil {
			values = many
		}
	}
	var resolved []string
	for _, value := range values {
		if normalized := normalizeShortcutKeys(value); normalized != "" {
			resolved = append(resolved, normalized)
		}
	}
	if len(resolved) == 0 {
		return []string{defaultQuickPanelShortcut()}
	}
	return resolved
}

// shortcutBinder binds one physical-key shortcut to the OS. unregister of an unbound shortcut succeeds.
type shortcutBinder interface {
	register(shortcut string) error
	unregister(shortcut string) error
}

// updateShortcuts replaces the registered set: drop the old bindings, defensively drop the new ones (a binding left
// behind by an earlier partial update), register the new ones, and on the first failure undo the new registrations
// and restore the old ones. The failure is returned; rollback errors are only logged.
func updateShortcuts(binder shortcutBinder, old, next []string) error {
	for _, shortcut := range old {
		if err := binder.unregister(shortcut); err != nil {
			log.Printf("failed to unregister the old global shortcut %q: %v", shortcut, err)
		}
	}
	for _, shortcut := range next {
		if !contains(old, shortcut) {
			_ = binder.unregister(shortcut)
		}
	}
	for i, shortcut := range next {
		err := binder.register(shortcut)
		if err == nil {
			continue
		}
		log.Printf("registering the global shortcut %q failed, rolling back: %v", shortcut, err)
		for _, done := range next[:i] {
			_ = binder.unregister(done)
		}
		for _, previous := range old {
			if rollbackErr := binder.register(previous); rollbackErr != nil {
				log.Printf("failed to restore the global shortcut %q: %v", previous, rollbackErr)
			}
		}
		return err
	}
	return nil
}

func contains(list []string, want string) bool {
	for _, item := range list {
		if item == want {
			return true
		}
	}
	return false
}

// wailsShortcutBinder registers shortcuts with app.GlobalShortcut. Wails binds one accelerator per call, so a
// two-step chord registers both steps and the press handler keeps the pending-chord state, as the Tauri shell does.
type wailsShortcutBinder struct {
	manager   *application.GlobalShortcutManager
	onPressed func()

	mu      sync.Mutex
	pending *pendingChord
}

type pendingChord struct {
	second  string
	armedAt time.Time
}

func (b *wailsShortcutBinder) register(shortcut string) error {
	segments := strings.Fields(shortcut)
	switch len(segments) {
	case 1:
		return b.registerCombo(segments[0], func() {
			log.Printf("global shortcut triggered: %s", segments[0])
			b.onPressed()
		})
	case 2:
		leader, second := segments[0], segments[1]
		if err := b.registerCombo(leader, func() { b.leaderPressed(leader, second) }); err != nil {
			return err
		}
		if second != leader { // a same-key chord is a double tap, fully handled by the leader's handler
			if err := b.registerCombo(second, func() { b.secondPressed(second) }); err != nil {
				_ = b.manager.Unregister(leader)
				return err
			}
		}
	}
	return nil
}

func (b *wailsShortcutBinder) unregister(shortcut string) error {
	for _, combo := range strings.Fields(shortcut) {
		_ = b.manager.Unregister(combo) // not registered counts as success
	}
	return nil
}

// registerCombo drops a leftover binding of the same accelerator first: Wails reports a duplicate registration as
// an error and keeps the old callback, which would make a changed handler silently ineffective.
func (b *wailsShortcutBinder) registerCombo(combo string, onPress func()) error {
	_ = b.manager.Unregister(combo)
	return b.manager.Register(combo, onPress)
}

func (b *wailsShortcutBinder) leaderPressed(leader, second string) {
	b.mu.Lock()
	if second == leader && b.pending != nil && b.pending.second == second && time.Since(b.pending.armedAt) <= chordWindow {
		b.pending = nil
		b.mu.Unlock()
		log.Printf("global chord (double tap) triggered: %s", leader)
		b.onPressed()
		return
	}
	b.pending = &pendingChord{second: second, armedAt: time.Now()}
	b.mu.Unlock()
}

func (b *wailsShortcutBinder) secondPressed(second string) {
	b.mu.Lock()
	fires := b.pending != nil && b.pending.second == second && time.Since(b.pending.armedAt) <= chordWindow
	if fires {
		b.pending = nil
	}
	b.mu.Unlock()
	if fires {
		log.Printf("global chord (leader+key) triggered: %s", second)
		b.onPressed()
	}
}

// applyOSShortcuts moves the registered set to next and records it. The caller holds shortcutsMu.
func (h *HostService) applyOSShortcuts(next []string) error {
	if !shortcutBackendAllowed() {
		h.osShortcuts = next
		return nil
	}
	if err := updateShortcuts(h.shortcutBinder(), h.osShortcuts, next); err != nil {
		return hostapi.New(hostapi.CodeConflict, err.Error())
	}
	h.osShortcuts = next
	return nil
}

func (h *HostService) shortcutBinder() shortcutBinder {
	if h.binder == nil {
		h.binder = &wailsShortcutBinder{manager: h.app.GlobalShortcut, onPressed: h.requestPanelToggle}
	}
	return h.binder
}

// panelShortcutTarget is the set that must be registered with the OS: the configured shortcuts while the quick
// panel is enabled, none otherwise (a disabled panel holds no global shortcut).
func panelShortcutTarget(enabled bool, shortcuts map[string]json.RawMessage) []string {
	if !enabled {
		return nil
	}
	return resolveQuickPanelShortcuts(shortcuts)
}

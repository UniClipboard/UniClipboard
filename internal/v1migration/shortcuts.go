package v1migration

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"maps"
	"os"
	"path/filepath"
	"slices"
	"strings"

	"github.com/BurntSushi/toml"

	"github.com/uniclipboard/unic/internal/desktopkeys"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

type legacyShortcutSettings struct {
	Bindings map[string]json.RawMessage
	Enabled  *bool
	Modifier string
}

func readLegacyShortcuts(root string) (legacyShortcutSettings, error) {
	for _, file := range []struct{ name, shortcuts, panel, modifier string }{
		{"desktop-settings.json", "keyboardShortcuts", "quickPanel", "doubleTapModifier"},
		{"settings.json", "keyboard_shortcuts", "quick_panel", "double_tap_modifier"},
	} {
		path := filepath.Join(root, file.name)
		if _, err := os.Stat(path); errors.Is(err, os.ErrNotExist) {
			continue
		}
		data, err := readBounded(path, 1<<20)
		if err != nil {
			return legacyShortcutSettings{}, fmt.Errorf("read v1 shortcut settings: %w", err)
		}
		var document map[string]json.RawMessage
		if err := json.Unmarshal(data, &document); err != nil {
			return legacyShortcutSettings{}, errors.New("invalid v1 shortcut settings document")
		}
		var result legacyShortcutSettings
		if raw := document[file.shortcuts]; raw != nil && json.Unmarshal(raw, &result.Bindings) != nil {
			return legacyShortcutSettings{}, errors.New("invalid v1 shortcut settings")
		}
		if raw := document[file.panel]; raw != nil {
			var panel map[string]json.RawMessage
			if json.Unmarshal(raw, &panel) != nil {
				return legacyShortcutSettings{}, errors.New("invalid v1 quick panel settings")
			}
			if raw := panel["enabled"]; raw != nil {
				_ = json.Unmarshal(raw, &result.Enabled)
			}
			if raw := panel[file.modifier]; raw != nil {
				_ = json.Unmarshal(raw, &result.Modifier)
			}
		}
		return result, nil
	}
	return legacyShortcutSettings{}, nil
}

func (s *Source) importShortcuts(target string, report *Report) error {
	legacy, err := readLegacyShortcuts(s.root)
	if err != nil {
		return err
	}
	if len(legacy.Bindings) == 0 && legacy.Enabled == nil && legacy.Modifier == "" {
		return nil
	}
	path := filepath.Join(target, "config.toml")
	manager, err := config.Open(path, nil)
	if err != nil {
		return err
	}
	state := manager.State()
	cfg := state.Config
	next := make(map[string][]string, len(cfg.Desktop.Shortcuts)+len(legacy.Bindings))
	maps.Copy(next, cfg.Desktop.Shortcuts)
	rawConfig, err := os.ReadFile(path)
	if err != nil {
		return err
	}
	metadata, err := toml.Decode(string(rawConfig), new(map[string]any))
	if err != nil {
		return err
	}
	preservePanel := metadata.IsDefined("desktop", "quick_panel_shortcut")
	if legacy.Enabled != nil && !*legacy.Enabled {
		legacy.Bindings = maps.Clone(legacy.Bindings)
		if legacy.Bindings == nil {
			legacy.Bindings = map[string]json.RawMessage{}
		}
		legacy.Bindings[desktopkeys.TogglePanel] = json.RawMessage("[]")
	}
	actions := desktopkeys.ActionsForPlatform()
	for _, id := range slices.Sorted(maps.Keys(legacy.Bindings)) {
		if _, ok := next[id]; ok || (id == desktopkeys.TogglePanel && preservePanel) {
			continue
		}
		if !slices.ContainsFunc(actions, func(action desktopkeys.Action) bool { return action.ID == id }) {
			report.ShortcutsSkipped++
			continue
		}
		raw := legacy.Bindings[id]
		values := []string{}
		var single string
		if bytes.Equal(raw, []byte("null")) {
			continue
		} else if json.Unmarshal(raw, &single) == nil {
			values = append(values, single)
		} else if json.Unmarshal(raw, &values) != nil {
			report.ShortcutsSkipped++
			continue
		}
		keys := []string{}
		for _, value := range values {
			if strings.TrimSpace(value) == "" {
				continue
			}
			key, err := desktopkeys.Normalize(value)
			if err != nil || desktopkeys.Physical(key) == "" {
				report.ShortcutsSkipped++
				continue
			}
			if !slices.Contains(keys, key) {
				keys = append(keys, key)
			}
		}
		if len(values) != 0 && len(keys) == 0 && slices.ContainsFunc(values, func(value string) bool { return strings.TrimSpace(value) != "" }) {
			continue
		}
		if id == desktopkeys.TogglePanel && len(keys) == 0 && (legacy.Enabled == nil || *legacy.Enabled) {
			continue // Empty v1 global binding selected its built-in default.
		}
		next[id] = keys
		report.ShortcutsImported++
	}
	cfg.Desktop.Shortcuts = next
	if !metadata.IsDefined("desktop", "quick_panel_double_tap") {
		switch legacy.Modifier {
		case "alt", "control", "meta":
			cfg.Desktop.QuickPanelDoubleTap = legacy.Modifier
		}
	}
	if err := cfg.Validate(); err != nil {
		// Keep all explicit user bindings. Unbind only overlapping new defaults.
		for _, id := range []string{desktopkeys.All, desktopkeys.Favorites, desktopkeys.Devices, desktopkeys.Layout, desktopkeys.Tags,
			desktopkeys.Actions, desktopkeys.Send, desktopkeys.PanelFavorite, desktopkeys.PanelActions, desktopkeys.PanelClear} {
			if _, ok := next[id]; ok {
				continue
			}
			for _, action := range actions {
				if action.ID == id && collidesWithExplicit(action, actions, next) {
					next[id] = []string{}
				}
			}
		}
	}
	if report.ShortcutsImported == 0 && cfg.Desktop.QuickPanelDoubleTap == state.Config.Desktop.QuickPanelDoubleTap {
		return nil
	}
	if err := cfg.Validate(); err != nil {
		return fmt.Errorf("migrate v1 shortcuts: %w", err)
	}
	var buf bytes.Buffer
	if err := toml.NewEncoder(&buf).Encode(cfg); err != nil {
		return err
	}
	_, err = manager.Replace(state.Revision, buf.Bytes())
	return err
}

func collidesWithExplicit(action desktopkeys.Action, actions []desktopkeys.Action, overrides map[string][]string) bool {
	for _, other := range actions {
		values, explicit := overrides[other.ID]
		if !explicit {
			continue
		}
		for _, fallback := range action.Defaults {
			key := desktopkeys.Physical(fallback)
			for _, value := range values {
				value = desktopkeys.Physical(value)
				if desktopkeys.Overlap(action.Scope, key, other.Scope, value) {
					return true
				}
			}
		}
	}
	return false
}

package v1migration

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"

	"github.com/BurntSushi/toml"

	config "github.com/uniclipboard/unic/internal/localconfig"
	"github.com/uniclipboard/unic/plugin"
)

type preferences struct {
	theme            string
	name             string
	analytics        *bool
	sidebar          map[string]bool
	protectFavorites bool
}

func (s *Source) preferences(report *Report) preferences {
	var result preferences
	read := func(name string, dest any) bool {
		path := filepath.Join(s.root, name)
		if _, err := os.Stat(path); errors.Is(err, os.ErrNotExist) {
			return false
		}
		data, err := readBounded(path, 1<<20)
		if err != nil || json.Unmarshal(data, dest) != nil {
			report.PreferenceFailures++
			return false
		}
		return true
	}
	var legacy struct {
		General struct {
			DeviceName string `json:"device_name"`
			Theme      string `json:"theme"`
			Analytics  *bool  `json:"usage_analytics_enabled"`
		} `json:"general"`
		RetentionPolicy struct {
			SkipPinned *bool `json:"skip_pinned"`
		} `json:"retention_policy"`
	}
	if read("settings.json", &legacy) {
		result = preferences{theme: legacy.General.Theme, name: strings.TrimSpace(legacy.General.DeviceName), analytics: legacy.General.Analytics}
	}
	if s.favoriteColumn {
		// v1 defaults to protecting favorites when the field is absent.
		skipPinned := legacy.RetentionPolicy.SkipPinned
		result.protectFavorites = skipPinned == nil || *skipPinned
	}
	var desktop struct {
		Version int `json:"version"`
		General struct {
			Theme     *string `json:"theme"`
			Analytics *bool   `json:"usageAnalyticsEnabled"`
		} `json:"general"`
	}
	if read("desktop-settings.json", &desktop) {
		if desktop.Version != 1 {
			report.PreferenceFailures++
		} else {
			if desktop.General.Theme != nil {
				result.theme = *desktop.General.Theme
			}
			if desktop.General.Analytics != nil {
				result.analytics = desktop.General.Analytics
			}
		}
	}
	switch result.theme {
	case "", "system", "light", "dark":
	default:
		result.theme = ""
		report.PreferenceFailures++
	}
	if len(result.name) > 64 || strings.ContainsAny(result.name, "\r\n\x00") {
		result.name = ""
		report.PreferenceFailures++
	}
	return result
}

// importPreferences fills absent fields in the saved TOML. Encoding the effective
// config would turn defaults into explicit settings, preventing later recovery.
func importPreferences(ctx context.Context, target string, colors map[string]string, prefs preferences, report *Report) {
	hasPrefs := prefs.theme != "" || prefs.name != "" || prefs.analytics != nil || len(prefs.sidebar) > 0 || prefs.protectFavorites
	if !hasPrefs && len(colors) == 0 {
		return
	}
	failed := func() {
		if len(colors) > 0 {
			report.ColorFailures++
		}
		if hasPrefs {
			report.PreferenceFailures++
		}
	}

	path := filepath.Join(target, "config.toml")
	manager, err := config.Open(path, nil)
	if err != nil {
		failed()
		return
	}
	state := manager.State()
	raw, err := os.ReadFile(path)
	sum := sha256.Sum256(raw)
	if err != nil || hex.EncodeToString(sum[:]) != state.Revision {
		failed()
		return
	}
	var fields map[string]any
	if _, err := toml.Decode(string(raw), &fields); err != nil {
		failed()
		return
	}
	setMissing := func(value any, path ...string) bool {
		parent := fields
		for _, part := range path[:len(path)-1] {
			child, ok := parent[part].(map[string]any)
			if !ok {
				child = map[string]any{}
				parent[part] = child
			}
			parent = child
		}
		key := path[len(path)-1]
		if _, ok := parent[key]; ok {
			return false
		}
		parent[key] = value
		return true
	}

	expectedColors := make(map[string]string, len(colors))
	for id, color := range colors {
		key := migrationColorKey(id)
		if setMissing(color, "desktop", "tag_colors", key) {
			report.ColorsMissing++
		} else {
			report.ColorsPreserved++
			color = state.Config.Desktop.TagColors[key]
		}
		expectedColors[key] = color
	}

	imported := 0
	add := func(value any, path ...string) {
		if setMissing(value, path...) {
			imported++
		}
	}
	if prefs.theme != "" {
		add(prefs.theme, "desktop", "color_mode")
	}
	if prefs.name != "" {
		add(prefs.name, "device_name")
	}
	if prefs.analytics != nil {
		add(*prefs.analytics, "telemetry", "analytics", "enabled")
	}
	if prefs.protectFavorites {
		add([]string{plugin.TagFavorite}, "history", "excluded_tags")
	}
	for id, visible := range prefs.sidebar {
		add(visible, "desktop", "tag_sidebar", migrationColorKey(id))
	}
	if report.ColorsMissing > 0 || imported > 0 {
		var encoded bytes.Buffer
		if ctx.Err() != nil || toml.NewEncoder(&encoded).Encode(fields) != nil {
			failed()
			return
		}
		if _, err := manager.Replace(state.Revision, encoded.Bytes()); err != nil {
			failed()
			return
		}
		raw = encoded.Bytes()
		report.ColorsImported, report.ColorsMissing = report.ColorsMissing, 0
	}
	persisted, err := os.ReadFile(path)
	if err != nil || !bytes.Equal(persisted, raw) {
		failed()
		return
	}
	cfg, err := config.Parse(persisted)
	if err != nil {
		failed()
		return
	}
	for key, want := range expectedColors {
		if cfg.Desktop.TagColors[key] != want {
			report.ColorFailures++
		} else {
			report.ColorsVerified++
		}
	}
	report.PreferencesImported += imported
}

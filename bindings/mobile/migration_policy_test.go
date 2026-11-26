package mobile

import (
	"encoding/json"
	"os"
	"path/filepath"
	"slices"
	"testing"
)

func TestInitializeLegacyHistoryPolicy(t *testing.T) {
	for _, tc := range []struct {
		name, input          string
		unchanged, wantError bool
	}{
		{name: "missing settings"},
		{name: "existing unrelated settings", input: `{"device_name":"Phone","history":{"max_items":42,"excluded_tags":["keep"],"future":true},"future":{"v":1}}`},
		{name: "explicit user policy", input: `{"history":{"auto_cleanup":true,"retention_days":30,"excluded_tags":[]}}`, unchanged: true},
		{name: "explicit disabled policy", input: `{"history":{"auto_cleanup":false,"retention_days":14}}`, unchanged: true},
		{name: "invalid settings are not overwritten", input: `{"history":{"auto_cleanup":"yes"}}`, unchanged: true, wantError: true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			dir := t.TempDir()
			path := filepath.Join(dir, mobileSettingsFilename)
			if tc.input != "" {
				if err := os.WriteFile(path, []byte(tc.input), 0o600); err != nil {
					t.Fatal(err)
				}
			}
			if err := initializeLegacyHistoryPolicy(dir); (err != nil) != tc.wantError {
				t.Fatalf("initialize policy: %v, want error: %v", err, tc.wantError)
			}
			raw, err := os.ReadFile(path)
			if err != nil {
				t.Fatal(err)
			}
			if tc.unchanged {
				if string(raw) != tc.input {
					t.Fatalf("settings changed: %s", raw)
				}
				return
			}
			settings, err := loadSettingsOrDefault(path, engineConfigDTO{})
			if err != nil {
				t.Fatal(err)
			}
			h := settings.History
			if h.AutoCleanup || h.RetentionDays != 0 || !slices.Contains(h.ExcludedTags, "starred") || !slices.Contains(h.ExcludedTags, "pinned") {
				t.Fatalf("unsafe initial cleanup policy: %+v", h)
			}
			if tc.input != "" {
				if settings.DeviceName != "Phone" || h.MaxItems != 42 || !slices.Contains(h.ExcludedTags, "keep") {
					t.Fatalf("existing settings were lost: %+v", settings)
				}
				var extra struct {
					Future  json.RawMessage
					History struct{ Future bool }
				}
				if err := json.Unmarshal(raw, &extra); err != nil || len(extra.Future) == 0 || !extra.History.Future {
					t.Fatalf("unrelated settings were lost: %s", raw)
				}
			}
			if err := initializeLegacyHistoryPolicy(dir); err != nil {
				t.Fatal(err)
			}
			if retried, err := os.ReadFile(path); err != nil || string(retried) != string(raw) {
				t.Fatalf("retry changed settings: %s (%v)", retried, err)
			}
		})
	}
}

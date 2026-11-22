package luascripts

import (
	"os"
	"path/filepath"
	"testing"
)

const base = `manifest_version = 1
kind = "lua"
entry = "main.lua"
id = "t.p"
version = "1.0.0"
api_version = 1
capabilities = ["processor"]
`

func mkPlugin(t *testing.T, extra string) string {
	t.Helper()
	root := t.TempDir()
	d := filepath.Join(root, "p")
	if err := os.MkdirAll(d, 0o700); err != nil {
		t.Fatal(err)
	}
	for f, s := range map[string]string{"spec.toml": base + extra, "main.lua": "return {}"} {
		if err := os.WriteFile(filepath.Join(d, f), []byte(s), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	return root
}

// on_error and config are the user's, in config.toml: a manifest that carries
// them is refused rather than silently ignored.
func TestManifestRefusesUserSettings(t *testing.T) {
	for _, extra := range []string{"on_error = \"block_sync\"\n", "[config]\nk = 1\n"} {
		if _, errs := Scan(mkPlugin(t, extra)); len(errs) != 1 {
			t.Errorf("%q: manifest accepted", extra)
		}
	}
}

func TestSpecsTakeSettingsFromTheHost(t *testing.T) {
	root := mkPlugin(t, "")
	specs, _, err := Specs(root, []string{"t.p"}, func(string) (string, map[string]any) {
		return "block_sync", map[string]any{"tag": "x"}
	})
	if err != nil || len(specs) != 1 || specs[0].OnError != "block_sync" || specs[0].Config["tag"] != "x" {
		t.Fatalf("%+v %v", specs, err)
	}
}

func TestManifestDecodesActionPlacements(t *testing.T) {
	root := mkPlugin(t, `
[[actions]]
id = "share"
title = "Share"
accepts = ["text", "image"]
placements = ["history.item_menu", "history.toolbar"]
post_actions = ["copy", "present"]
timeout_ms = 60000
`)
	found, errs := Scan(root)
	if len(errs) != 0 || len(found) != 1 {
		t.Fatalf("found=%+v errs=%+v", found, errs)
	}
	spec := found["t.p"].Manifest.PluginSpec()
	if len(spec.Actions) != 1 || spec.Actions[0].ID != "share" ||
		len(spec.Actions[0].Placements) != 2 ||
		spec.Actions[0].Placements[0] != "history.item_menu" ||
		spec.Actions[0].Placements[1] != "history.toolbar" ||
		len(spec.Actions[0].PostActions) != 2 ||
		spec.Actions[0].PostActions[0] != "copy" ||
		spec.Actions[0].PostActions[1] != "present" {
		t.Fatalf("unexpected actions: %+v", spec.Actions)
	}
}

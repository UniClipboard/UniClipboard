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

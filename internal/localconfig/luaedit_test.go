package config

import (
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
)

func editFile(t *testing.T, body string, id string, on bool) (string, bool, Config) {
	t.Helper()
	p := filepath.Join(t.TempDir(), "config.toml")
	if err := os.WriteFile(p, []byte(body), 0o600); err != nil {
		t.Fatal(err)
	}
	ch, err := SetLuaEnabled(p, id, on)
	if err != nil {
		t.Fatal(err)
	}
	b, _ := os.ReadFile(p)
	c, err := Parse(b)
	if err != nil {
		t.Fatal(err)
	}
	return string(b), ch, c
}

func TestSetLuaEnabledKeepsTheRestOfTheFile(t *testing.T) {
	cases := map[string]string{
		"none":      "device_name = \"x\" # mine\n",
		"commented": "# [plugins]\n# lua = [\"old\"]\n[sync]\nenabled = true\n",
		"table":     "[plugins]\non_error = \"continue\"\n",
		"inline":    "[plugins]\nlua = [\"a\"] # keep order\non_error = \"continue\"\n",
		"multiline": "[plugins]\nlua = [\n  \"a\", # first\n  \"b\",\n]\non_error = \"continue\"\n",
	}
	for name, body := range cases {
		out, ch, c := editFile(t, body, "new.p", true)
		if !ch || !slices.Contains(c.Plugins.Lua, "new.p") {
			t.Fatalf("%s: %v %q", name, ch, out)
		}
		for _, keep := range []string{"# mine", "on_error", "[sync]"} {
			if strings.Contains(body, keep) && !strings.Contains(out, keep) {
				t.Fatalf("%s: lost %q in\n%s", name, keep, out)
			}
		}
		out, ch, c = editFile(t, out, "new.p", false)
		if !ch || slices.Contains(c.Plugins.Lua, "new.p") {
			t.Fatalf("%s: disable: %q", name, out)
		}
	}
}

func TestSetLuaEnabledIsIdempotentAndRefusesBadConfig(t *testing.T) {
	if _, ch, _ := editFile(t, "[plugins]\nlua = [\"a\"]\n", "a", true); ch {
		t.Fatal("enabling twice changed the file")
	}
	if _, ch, _ := editFile(t, "", "a", false); ch {
		t.Fatal("disabling an absent id changed the file")
	}
	p := filepath.Join(t.TempDir(), "config.toml")
	_ = os.WriteFile(p, []byte("bogus = ["), 0o600)
	if _, err := SetLuaEnabled(p, "a", true); err == nil {
		t.Fatal("edited an invalid file")
	}
}

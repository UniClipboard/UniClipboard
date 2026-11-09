package main

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func mkLua(t *testing.T, root, dir, id string) {
	t.Helper()
	d := filepath.Join(root, dir)
	_ = os.MkdirAll(d, 0o700)
	spec := "manifest_version = 1\nid = \"" + id + "\"\nversion = \"1\"\napi_version = 1\nkind = \"lua\"\nentry = \"main.lua\"\ncapabilities = [\"processor\"]\nevents = [\"clipboard.captured\"]\n"
	_ = os.WriteFile(filepath.Join(d, "spec.toml"), []byte(spec), 0o600)
	_ = os.WriteFile(filepath.Join(d, "main.lua"), []byte("-- x\n"), 0o600)
}

func plug(t *testing.T, root, cfg string, args ...string) (string, error) {
	var b bytes.Buffer
	err := pluginCmd(args, root, cfg, &b)
	return b.String(), err
}

func TestPluginDroppedInIsFoundButNotEnabled(t *testing.T) {
	root, cfg := filepath.Join(t.TempDir(), "lua"), filepath.Join(t.TempDir(), "config.toml")
	mkLua(t, root, "tagger", "me.tagger")
	_ = os.MkdirAll(filepath.Join(root, "broken"), 0o700)
	_ = os.WriteFile(filepath.Join(root, "broken", "spec.toml"), []byte("nope ="), 0o600)

	out, err := plug(t, root, cfg, "list")
	if err != nil || !strings.Contains(out, "me.tagger") || !strings.Contains(out, "disabled") || !strings.Contains(out, "not usable") {
		t.Fatalf("%q %v", out, err)
	}
	if _, err := plug(t, root, cfg, "enable", "me.tagger"); err != nil {
		t.Fatal(err)
	}
	if out, _ = plug(t, root, cfg, "list"); !strings.Contains(out, "enabled") || strings.Contains(out, "disabled") {
		t.Fatalf("after enable: %q", out)
	}
	if out, _ = plug(t, root, cfg, "enable", "me.tagger"); !strings.Contains(out, "already") {
		t.Fatalf("%q", out)
	}
	if _, err := plug(t, root, cfg, "disable", "me.tagger"); err != nil {
		t.Fatal(err)
	}
	if out, _ = plug(t, root, cfg, "list"); !strings.Contains(out, "disabled") {
		t.Fatalf("after disable: %q", out)
	}
	if _, err := plug(t, root, cfg, "enable", "ghost"); err == nil {
		t.Fatal("enabled a plugin that is not there")
	}
}

package luascripts

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/uniclipboard/unic/plugin"
)

func TestManifestProcessorStageValidation(t *testing.T) {
	write := func(t *testing.T, tomlBody string) error {
		t.Helper()
		dir := t.TempDir()
		if err := os.WriteFile(filepath.Join(dir, ManifestFile), []byte(tomlBody), 0o600); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(dir, "main.lua"), []byte("return {}"), 0o600); err != nil {
			t.Fatal(err)
		}
		_, err := Read(dir)
		return err
	}

	okGuard := `
manifest_version = 1
id = "example.guard"
version = "1.0.0"
api_version = 1
kind = "lua"
entry = "main.lua"
capabilities = ["processor"]
processor_stage = "guard"
`
	if err := write(t, okGuard); err != nil {
		t.Fatalf("valid guard manifest rejected: %v", err)
	}

	badStage := `
manifest_version = 1
id = "example.bad"
version = "1.0.0"
api_version = 1
kind = "lua"
entry = "main.lua"
capabilities = ["processor"]
processor_stage = "pre"
`
	if err := write(t, badStage); err == nil {
		t.Fatal("expected invalid processor_stage to be rejected")
	}

	nonProc := `
manifest_version = 1
id = "example.obs"
version = "1.0.0"
api_version = 1
kind = "lua"
entry = "main.lua"
capabilities = ["observer"]
processor_stage = "guard"
`
	if err := write(t, nonProc); err == nil {
		t.Fatal("expected processor_stage without processor capability to be rejected")
	}

	m := Manifest{ID: "example.guard", Version: "1.0.0", APIVersion: 1, Capabilities: []string{plugin.CapProcessor}, ProcessorStage: plugin.ProcessorStageGuard}
	if m.PluginSpec().ProcessorStage != plugin.ProcessorStageGuard {
		t.Fatalf("PluginSpec().ProcessorStage = %q, want guard", m.PluginSpec().ProcessorStage)
	}
	a := Script{Manifest: Manifest{ID: "example.p", Version: "1.0.0", APIVersion: 1, Capabilities: []string{plugin.CapProcessor}}}
	b := Script{Manifest: Manifest{ID: "example.p", Version: "1.0.0", APIVersion: 1, Capabilities: []string{plugin.CapProcessor}, ProcessorStage: plugin.ProcessorStageAnnotate}}
	c := Script{Manifest: Manifest{ID: "example.p", Version: "1.0.0", APIVersion: 1, Capabilities: []string{plugin.CapProcessor}, ProcessorStage: plugin.ProcessorStageGuard}}
	if !a.Same(b) {
		t.Fatal("omitted and annotate processor stages should be equivalent in Script.Same")
	}
	if a.Same(c) {
		t.Fatal("guard and annotate processor stages must not be equal in Script.Same")
	}
}
